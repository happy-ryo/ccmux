import { Footer, Layout, Navbar } from 'nextra-theme-docs'
import { Head } from 'nextra/components'
import { getPageMap } from 'nextra/page-map'
import 'nextra-theme-docs/style.css'
import './globals.css'

const siteUrl = 'https://happy-ryo.github.io/ccmux/docs'

export const metadata = {
  title: 'ccmux-fork — Claude Code Multiplexer (fork)',
  description: 'Fork of ccmux with independent development. Manage multiple Claude Code instances in TUI split panes.',
  openGraph: {
    title: 'ccmux-fork — Claude Code Multiplexer (fork)',
    description: 'Fork of ccmux with independent development. Rust-powered terminal multiplexer with tabs, file tree, and syntax-highlighted preview.',
    url: siteUrl,
    siteName: 'ccmux-fork',
    type: 'website',
  },
  twitter: {
    card: 'summary',
    title: 'ccmux-fork — Claude Code Multiplexer (fork)',
    description: 'Fork of ccmux with independent development.',
  },
}

export const viewport = {
  width: 'device-width',
  initialScale: 1,
}

const logo = <span style={{ fontWeight: 800, fontSize: '1.1rem' }}>◈ ccmux</span>

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="ja" dir="ltr" suppressHydrationWarning>
      <Head />
      <body>
        <Layout
          navbar={
            <Navbar
              logo={logo}
              projectLink="https://github.com/happy-ryo/ccmux"
            />
          }
          pageMap={await getPageMap()}
          docsRepositoryBase="https://github.com/happy-ryo/ccmux/tree/main/docs"
          footer={<Footer>MIT License · <a href="https://github.com/happy-ryo/ccmux" target="_blank" rel="noopener" style={{color: '#d97757'}}>ccmux-fork</a>, a fork of <a href="https://github.com/Shin-sibainu/ccmux" target="_blank" rel="noopener" style={{color: '#d97757'}}>ccmux</a></Footer>}
        >
          {children}
        </Layout>
      </body>
    </html>
  )
}
