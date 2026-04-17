import { Footer, Layout, Navbar } from 'nextra-theme-docs'
import { Head } from 'nextra/components'
import { getPageMap } from 'nextra/page-map'
import 'nextra-theme-docs/style.css'
import './globals.css'

const siteUrl = 'https://shin-sibainu.github.io/ccmux/docs'

export const metadata = {
  title: 'ccmux — Claude Code Multiplexer',
  description: 'Manage multiple Claude Code instances in TUI split panes',
  openGraph: {
    title: 'ccmux — Claude Code Multiplexer',
    description: 'Manage multiple Claude Code instances in TUI split panes. Rust-powered terminal multiplexer with tabs, file tree, and syntax-highlighted preview.',
    url: siteUrl,
    siteName: 'ccmux',
    images: [{ url: `${siteUrl}/og.png`, width: 1200, height: 630, alt: 'ccmux — Claude Code Multiplexer' }],
    type: 'website',
  },
  twitter: {
    card: 'summary_large_image',
    title: 'ccmux — Claude Code Multiplexer',
    description: 'Manage multiple Claude Code instances in TUI split panes.',
    images: [`${siteUrl}/og.png`],
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
              projectLink="https://github.com/Shin-sibainu/ccmux"
            />
          }
          pageMap={await getPageMap()}
          docsRepositoryBase="https://github.com/Shin-sibainu/ccmux/tree/master/docs"
          footer={<Footer>MIT License © ccmux — <a href="https://claude-code-academy.dev" target="_blank" rel="noopener" style={{color: '#d97757'}}>Claude Code Academy</a></Footer>}
        >
          {children}
        </Layout>
      </body>
    </html>
  )
}
