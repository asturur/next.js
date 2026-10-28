use anyhow::{Context, Result};
use tracing::{Instrument, Level, Span};
use turbo_rcstr::RcStr;
use turbo_tasks::{
    ReadRef, ResolvedVc, TryFlatJoinIterExt, TryJoinIterExt, ValueToString, Vc,
    graph::{AdjacencyMap, GraphTraversal, Visit},
};
use turbo_tasks_fs::{File, FileContent, FileSystem, FileSystemPath, glob::Glob};
use turbo_tasks_hash::HashAlgorithm;
use turbopack_core::{
    asset::{Asset, AssetContent},
    file_source::FileSource,
    module::Module,
    output::{OutputAsset, OutputAssets, OutputAssetsReference},
};

use crate::{
    nft::{EndpointTraceResult, tracing_exclude_glob},
    nft_json_builder::NftJsonBuilder,
    project::Project,
};

/// A json file that produces references to all files that are needed by the given module
/// at runtime. This will include, for example, node native modules, unanalyzable packages,
/// client side chunks, etc.
///
/// With this file, users can determine the minimum set of files that are needed alongside
/// their bundle.
#[turbo_tasks::value]
pub struct NftJsonAsset {
    project: ResolvedVc<Project>,
    /// The chunk for which the asset is being generated
    chunk: ResolvedVc<Box<dyn OutputAsset>>,
    /// Additional assets to include in the nft json. This can be used to manually collect assets
    /// that are known to be required but are not in the graph yet, for whatever reason.
    ///
    /// An example of this is the two-phase approach used by the `ClientReferenceManifest` in
    /// next.js.
    additional_assets: Vec<ResolvedVc<Box<dyn OutputAsset>>>,
    // The page name, e.g. `pages/index` or `app/route1`
    page_name: Option<RcStr>,

    traced_files: ResolvedVc<EndpointTraceResult>,
}

#[turbo_tasks::value_impl]
impl NftJsonAsset {
    #[turbo_tasks::function]
    pub fn new(
        project: ResolvedVc<Project>,
        page_name: Option<RcStr>,
        chunk: ResolvedVc<Box<dyn OutputAsset>>,
        additional_assets: Vec<ResolvedVc<Box<dyn OutputAsset>>>,
        traced_files: ResolvedVc<EndpointTraceResult>,
    ) -> Vc<Self> {
        NftJsonAsset {
            chunk,
            project,
            additional_assets,
            page_name,
            traced_files,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl OutputAssetsReference for NftJsonAsset {}

#[turbo_tasks::value_impl]
impl OutputAsset for NftJsonAsset {
    #[turbo_tasks::function]
    async fn path(&self) -> Result<Vc<FileSystemPath>> {
        let path = self.chunk.path().await?;
        Ok(path
            .fs
            .root()
            .await?
            .join(&format!("{}.nft.json", path.path))?
            .cell())
    }
}

#[turbo_tasks::value_impl]
impl Asset for NftJsonAsset {
    #[turbo_tasks::function]
    async fn content(self: Vc<Self>) -> Result<Vc<AssetContent>> {
        let this = &*self.await?;
        let span = tracing::info_span!(
            "output file tracing",
            path = display(self.path().to_string().await?)
        );
        async move {
            let project_path = this.project.project_path().owned().await?;

            let next_config = this.project.next_config();
            let hash_salt = next_config.output_hash_salt();

            let client_root = this.project.client_fs().root();
            let client_root = client_root.owned().await?;

            let nft_path = self.path().owned().await?;
            let mut nft_json = NftJsonBuilder::new(this.project, &nft_path).await?;

            let chunk = this.chunk;
            let entries = this
                .additional_assets
                .iter()
                .copied()
                .chain(std::iter::once(chunk))
                .collect();

            let exclude_glob =
                tracing_exclude_glob(this.page_name.clone(), project_path.clone(), next_config);

            enum AssetOrModule {
                Asset(ResolvedVc<Box<dyn OutputAsset>>),
                Module(ResolvedVc<Box<dyn Module>>),
            }

            // Collect referenced chunks (e.g. dynamic imports, etc).
            let all_assets = all_assets_from_entries_filtered(
                Vc::cell(entries),
                Some(client_root.clone()),
                exclude_glob.await?.map(|v| *v),
            )
            .await?;

            let traced_files = this.traced_files.await?;
            let module_data = traced_files.module_data.await?;

            let result = all_assets
                .iter()
                .filter(|a| **a != chunk)
                .copied()
                .map(AssetOrModule::Asset)
                .chain(
                    traced_files
                        .modules
                        .iter()
                        .copied()
                        .map(AssetOrModule::Module),
                )
                .map(async |referenced| {
                    let (referenced_chunk_path, hash, content) = match referenced {
                        AssetOrModule::Asset(v) => {
                            let content = v.content().to_resolved().await?;
                            (
                                v.path().owned().await?,
                                content
                                    .hash(hash_salt, HashAlgorithm::Xxh3Hash128Hex)
                                    .owned()
                                    .await?,
                                content.await?,
                            )
                        }
                        AssetOrModule::Module(v) => {
                            let ident = module_data
                                .idents
                                .get(&v)
                                .await?
                                .context("missing path for module")?;
                            let hash = module_data
                                .hashes
                                .get(&v)
                                .await?
                                .context("missing hash for module")?;
                            let source = v.source().await?.context("NFT module has no content")?;
                            (
                                ident.path.clone(),
                                (**hash).clone(),
                                source.content().await?,
                            )
                        }
                    };

                    if referenced_chunk_path.has_extension(".map") {
                        return Ok(None);
                    }

                    Ok(Some((referenced_chunk_path, hash, content)))
                })
                .try_flat_join()
                .await?;

            for (path, hash, content) in result {
                nft_json.add(path, hash, &content)?;
            }

            for file_path in &traced_files.includes {
                let content = FileSource::new(file_path.clone()).content();
                let hash = content
                    .hash(hash_salt, HashAlgorithm::Xxh3Hash128Hex)
                    .owned()
                    .await?;
                let content = content.await?;
                nft_json.add(file_path.clone(), hash, &content)?;
            }
            // We can't just add this into "files" because Next.js sometimes decides to delete
            // output files such as `.next/server/pages/index.js` if that page was prerendered and
            // is fully static. An alternative would be to postprocess the nft file so that
            // non-adapter consumers (which includes output:standalone) don't experience a breaking
            // change, but instead we just add it as a separate field that only build-complete
            // reads.
            let entry_hash = chunk
                .content()
                .hash(hash_salt, HashAlgorithm::Xxh3Hash128Hex)
                .owned()
                .await?;
            let json = serde_json::to_string(&nft_json.into_json(Some(entry_hash)))?;

            Ok(AssetContent::file(
                FileContent::Content(File::from(json)).cell(),
            ))
        }
        .instrument(span)
        .await
    }
}

/// Walks the asset graph from multiple assets and collect all referenced
/// assets, but filters out all client assets and glob matches.
#[turbo_tasks::function]
async fn all_assets_from_entries_filtered(
    entries: Vc<OutputAssets>,
    client_root: Option<FileSystemPath>,
    exclude_glob: Option<Vc<Glob>>,
) -> Result<Vc<OutputAssets>> {
    let exclude_glob = if let Some(exclude_glob) = exclude_glob {
        Some(exclude_glob.to_resolved().await?)
    } else {
        None
    };
    let emit_spans = tracing::enabled!(Level::INFO);
    Ok(Vc::cell(
        AdjacencyMap::new()
            .visit(
                entries
                    .await?
                    .iter()
                    .map(async |asset| {
                        Ok((
                            *asset,
                            if emit_spans {
                                // INVALIDATION: we don't need to invalidate the list of assets when
                                // the span name changes
                                Some(asset.path_string().untracked().await?)
                            } else {
                                None
                            },
                        ))
                    })
                    .try_join()
                    .await?,
                OutputAssetFilteredVisit {
                    client_root,
                    exclude_glob,
                    emit_spans,
                },
            )
            .await
            .completed()?
            .into_postorder_topological()
            .map(|n| n.0)
            .collect(),
    ))
}

struct OutputAssetFilteredVisit {
    client_root: Option<FileSystemPath>,
    exclude_glob: Option<ResolvedVc<Glob>>,
    emit_spans: bool,
}
impl Visit<(ResolvedVc<Box<dyn OutputAsset>>, Option<ReadRef<RcStr>>)>
    for OutputAssetFilteredVisit
{
    type EdgesIntoIter = Vec<(
        (ResolvedVc<Box<dyn OutputAsset>>, Option<ReadRef<RcStr>>),
        (),
    )>;
    type EdgesFuture = impl Future<Output = Result<Self::EdgesIntoIter>>;

    fn edges(
        &mut self,
        node: &(ResolvedVc<Box<dyn OutputAsset>>, Option<ReadRef<RcStr>>),
    ) -> Self::EdgesFuture {
        let client_root = self.client_root.clone();
        let exclude_glob: Option<ResolvedVc<Glob>> = self.exclude_glob.clone();
        get_referenced_server_assets(self.emit_spans, node.0, client_root, exclude_glob)
    }

    fn span(
        &mut self,
        node: &(ResolvedVc<Box<dyn OutputAsset>>, Option<ReadRef<RcStr>>),
        _edge: Option<&()>,
    ) -> tracing::Span {
        if let Some(ident) = &node.1 {
            tracing::trace_span!("asset", name = display(ident))
        } else {
            Span::current()
        }
    }
}

/// Computes the list of all chunk children of a given chunk, but filters out all client assets and
/// glob matches.
async fn get_referenced_server_assets(
    emit_spans: bool,
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    client_root: Option<FileSystemPath>,
    exclude_glob: Option<ResolvedVc<Glob>>,
) -> Result<
    Vec<(
        (ResolvedVc<Box<dyn OutputAsset>>, Option<ReadRef<RcStr>>),
        (),
    )>,
> {
    let refs =
        filtered_asset_references(*asset, client_root, exclude_glob.map(|glob| *glob)).await?;

    refs.iter()
        .map(async |asset| {
            Ok(Some((
                (
                    *asset,
                    if emit_spans {
                        // INVALIDATION: we don't need to invalidate the list of assets when the
                        // span name changes
                        Some(asset.path_string().untracked().await?)
                    } else {
                        None
                    },
                ),
                (),
            )))
        })
        .try_flat_join()
        .await
}

/// Cache direct, filtered edges per asset. Many endpoints share the same server chunks;
/// reading every descendant path in each endpoint's traversal repeats dependency
/// bookkeeping. A change to one asset only invalidates consumers of that asset's edges.
#[turbo_tasks::function]
async fn filtered_asset_references(
    asset: ResolvedVc<Box<dyn OutputAsset>>,
    client_root: Option<FileSystemPath>,
    exclude_glob: Option<ResolvedVc<Glob>>,
) -> Result<Vc<OutputAssets>> {
    let refs = asset.references().all_assets().await?;
    let exclude_glob = if let Some(glob) = exclude_glob {
        Some(glob.await?)
    } else {
        None
    };
    Ok(Vc::cell(
        refs.iter()
            .map(async |asset| {
                let asset_path = asset.path().await?;
                if client_root
                    .as_ref()
                    .is_some_and(|root| asset_path.is_inside_ref(root))
                    || exclude_glob
                        .as_ref()
                        .is_some_and(|glob| glob.matches(&asset_path.path))
                {
                    return Ok(None);
                }
                Ok(Some(*asset))
            })
            .try_flat_join()
            .await?,
    ))
}

#[cfg(test)]
mod filtered_edge_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyhow::Result;
    use turbo_rcstr::{RcStr, rcstr};
    use turbo_tasks::{ResolvedVc, State, Vc};
    use turbo_tasks_backend::{BackendOptions, TurboTasksBackend, noop_backing_storage};
    use turbo_tasks_fs::{
        File, FileContent, FileSystem, FileSystemPath, VirtualFileSystem,
        glob::{Glob, GlobOptions},
    };
    use turbopack_core::{
        asset::{Asset, AssetContent},
        output::{OutputAsset, OutputAssetsReference, OutputAssetsWithReferenced},
    };

    use super::filtered_asset_references;

    #[turbo_tasks::value]
    struct TestAsset {
        path: FileSystemPath,
        children: Vec<ResolvedVc<Box<dyn OutputAsset>>>,
        selected: State<Vec<usize>>,
    }

    #[turbo_tasks::value_impl]
    impl Asset for TestAsset {
        #[turbo_tasks::function]
        fn content(&self) -> Vc<AssetContent> {
            AssetContent::file(FileContent::Content(File::from("")).cell())
        }
    }

    #[turbo_tasks::value_impl]
    impl OutputAsset for TestAsset {
        #[turbo_tasks::function]
        fn path(&self) -> Vc<FileSystemPath> {
            self.path.clone().cell()
        }
    }

    #[turbo_tasks::value_impl]
    impl OutputAssetsReference for TestAsset {
        #[turbo_tasks::function]
        fn references(&self) -> Vc<OutputAssetsWithReferenced> {
            OutputAssetsWithReferenced::from_assets(Vc::cell(
                self.selected
                    .get()
                    .iter()
                    .map(|&index| self.children[index])
                    .collect(),
            ))
        }
    }

    static EXECUTIONS: AtomicUsize = AtomicUsize::new(0);

    #[turbo_tasks::value]
    struct ReadResult {
        paths: Vec<RcStr>,
        execution: usize,
    }

    #[turbo_tasks::function(operation, root)]
    async fn read_edges(
        asset: ResolvedVc<Box<dyn OutputAsset>>,
        client_root: FileSystemPath,
        exclude: ResolvedVc<Glob>,
    ) -> Result<Vc<ReadResult>> {
        let edges = filtered_asset_references(*asset, Some(client_root), Some(*exclude)).await?;
        let mut paths = Vec::new();
        for edge in edges.iter() {
            paths.push(edge.path().await?.path.clone());
        }
        Ok(ReadResult {
            paths,
            execution: EXECUTIONS.fetch_add(1, Ordering::SeqCst),
        }
        .cell())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn filtered_edges_preserve_exclusions_and_invalidate_only_affected_consumers() {
        let tt = turbo_tasks::TurboTasks::new(TurboTasksBackend::new(
            BackendOptions::default(),
            noop_backing_storage(),
        ));
        tt.run_once(async {
            let root = VirtualFileSystem::new_with_name(rcstr!("edges"))
                .root()
                .owned()
                .await?;
            let asset = |name: &str, children: Vec<ResolvedVc<Box<dyn OutputAsset>>>| {
                TestAsset {
                    path: root.join(name).unwrap(),
                    children,
                    selected: State::new(Vec::new()),
                }
                .resolved_cell()
            };
            let first = ResolvedVc::upcast(asset("server/first.js", vec![]));
            let second = ResolvedVc::upcast(asset("server/second.js", vec![]));
            let client = ResolvedVc::upcast(asset("client/page.js", vec![]));
            let excluded = ResolvedVc::upcast(asset("server/excluded.js", vec![]));
            let a = asset("server/a.js", vec![first, second, client, excluded]);
            let b = asset("server/b.js", vec![first, second]);
            a.await?.selected.set(vec![0, 2, 3]);
            b.await?.selected.set(vec![1]);
            let exclude = Glob::new(rcstr!("server/excluded.js"), GlobOptions::default())
                .to_resolved()
                .await?;
            let read_a = read_edges(ResolvedVc::upcast(a), root.join("client")?, exclude);
            let read_b = read_edges(ResolvedVc::upcast(b), root.join("client")?, exclude);
            let a_before = read_a.read_strongly_consistent().await?;
            let b_before = read_b.read_strongly_consistent().await?;
            assert_eq!(a_before.paths, [rcstr!("server/first.js")]);
            assert_eq!(b_before.paths, [rcstr!("server/second.js")]);
            b.await?.selected.set(vec![0]);
            let b_after = read_b.read_strongly_consistent().await?;
            let a_after = read_a.read_strongly_consistent().await?;
            assert_eq!(b_after.paths, [rcstr!("server/first.js")]);
            assert_ne!(b_after.execution, b_before.execution);
            assert_eq!(a_after.execution, a_before.execution);
            a.await?.selected.set(vec![1, 2, 3]);
            let a_changed = read_a.read_strongly_consistent().await?;
            assert_eq!(a_changed.paths, [rcstr!("server/second.js")]);
            assert_ne!(a_changed.execution, a_before.execution);
            assert_eq!(
                read_b.read_strongly_consistent().await?.execution,
                b_after.execution
            );
            Ok(())
        })
        .await
        .unwrap();
    }
}
