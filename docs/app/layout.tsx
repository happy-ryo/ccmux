import { Footer, Layout, Navbar } from 'nextra-theme-docs'
import { Head } from 'nextra/components'
import { getPageMap } from 'nextra/page-map'
import 'nextra-theme-docs/style.css'
import './globals.css'

const siteUrl = 'https://suisya-systems.github.io/renga/docs'

export const metadata = {
  title: 'renga — Claude Code Multiplexer',
  description: 'A terminal multiplexer purpose-built for running multiple Claude Code instances side-by-side. Peer messaging between Claude panes, IME-aware composition overlay, single Rust binary.',
  openGraph: {
    title: 'renga — Claude Code Multiplexer',
    description: 'Run multiple Claude Code sessions side-by-side. Peer messaging between Claude panes, IME-aware overlay, file tree, syntax-highlighted preview. Single Rust binary.',
    url: siteUrl,
    siteName: 'renga',
    type: 'website',
  },
  twitter: {
    card: 'summary',
    title: 'renga — Claude Code Multiplexer',
    description: 'Run multiple Claude Code sessions side-by-side with peer messaging and an IME-aware overlay.',
  },
}

export const viewport = {
  width: 'device-width',
  initialScale: 1,
}

const logo = <span style={{ fontWeight: 800, fontSize: '1.1rem' }}>◈ renga</span>

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ja" dir="ltr" suppressHydrationWarning>
      <Head />
      <body>
        <Layout
          navbar={
            <Navbar
              logo={logo}
              projectLink="https://github.com/suisya-systems/renga"
            />
          }
          pageMap={await getPageMap()}
          docsRepositoryBase="https://github.com/suisya-systems/renga/tree/main/docs"
          footer={<Footer>MIT License · <a href="https://github.com/suisya-systems/renga" target="_blank" rel="noopener" style={{color: '#d97757'}}>renga</a> · originally derived from <a href="https://github.com/Shin-sibainu/ccmux" target="_blank" rel="noopener" style={{color: '#d97757'}}>ccmux</a></Footer>}
        >
          {children}
        </Layout>
      </body>
    </html>
  )
}
