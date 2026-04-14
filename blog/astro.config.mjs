import { defineConfig } from 'astro/config';
import mdx from '@astrojs/mdx';
import sitemap from '@astrojs/sitemap';

const isProd = process.env.NODE_ENV === 'production';

export default defineConfig({
  site: 'https://shin-sibainu.github.io',
  base: isProd ? '/ccmux/blog' : '/',
  trailingSlash: 'always',
  integrations: [mdx(), sitemap()],
  build: {
    format: 'directory',
  },
  markdown: {
    shikiConfig: {
      theme: 'github-dark-dimmed',
      wrap: true,
    },
  },
});
