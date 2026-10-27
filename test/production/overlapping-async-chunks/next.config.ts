import type { NextConfig } from 'next'

export default {
  experimental: {
    turbopackClientSideNestedAsyncChunking: true,
  },
} satisfies NextConfig
