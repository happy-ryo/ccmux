import { defineConfig } from 'astro/config';
import mdx from '@astrojs/mdx';
import sitemap from '@astrojs/sitemap';
import expressiveCode from 'astro-expressive-code';

const isProd = process.env.NODE_ENV === 'production';

export default defineConfig({
  site: 'https://shin-sibainu.github.io',
  base: isProd ? '/ccmux/blog' : '/',
  trailingSlash: 'always',
  // expressive-code must come before mdx so its remark plugin sees
  // fenced code blocks first.
  integrations: [
    expressiveCode({
      themes: ['github-light'],
      defaultProps: {
        wrap: true,
      },
      styleOverrides: {
        borderRadius: '10px',
        borderColor: 'oklab(0.18 0 0 / 0.1)',
        codeFontFamily:
          "'JetBrains Mono', ui-monospace, 'SFMono-Regular', Menlo, Consolas, monospace",
        codeFontSize: '14px',
        codeLineHeight: '1.65',
        frames: {
          shadowColor: 'oklab(0.18 0 0 / 0.06)',
          editorTabBarBackground: '#ecebe5',
          terminalBackground: '#1c1b16',
          terminalTitlebarBackground: '#26251e',
          terminalTitlebarForeground: '#adbac7',
        },
      },
    }),
    mdx(),
    sitemap(),
  ],
  build: {
    format: 'directory',
  },
});
