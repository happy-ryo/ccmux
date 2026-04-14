import rss from '@astrojs/rss';
import { getCollection } from 'astro:content';
import type { APIContext } from 'astro';

export async function GET(context: APIContext) {
  const all = await getCollection('posts', ({ data }) => !data.draft);
  const posts = all.sort(
    (a, b) => b.data.pubDate.valueOf() - a.data.pubDate.valueOf()
  );

  return rss({
    title: 'ccmux blog',
    description:
      'ccmux の内部構造・設計判断・機能の使い方を綴るエンジニアブログ。',
    site: context.site!,
    items: posts.map((post) => ({
      title: post.data.title,
      pubDate: post.data.pubDate,
      description: post.data.description,
      link: `/${post.id.replace(/\.mdx?$/, '')}/`,
      categories: post.data.tags,
    })),
    customData: '<language>ja-jp</language>',
  });
}
