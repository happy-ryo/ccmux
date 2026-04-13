import { Footer, Layout, Navbar } from 'nextra-theme-docs'
import { Head } from 'nextra/components'
import { getPageMap } from 'nextra/page-map'
import 'nextra-theme-docs/style.css'
import './globals.css'

export const metadata = {
  title: 'ccmux — Claude Code Multiplexer',
  description: 'Manage multiple Claude Code instances in TUI split panes',
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
          footer={<Footer>MIT License © ccmux</Footer>}
        >
          {children}
        </Layout>
      </body>
    </html>
  )
}
