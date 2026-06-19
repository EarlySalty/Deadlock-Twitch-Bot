import { Navbar } from '@/components/layout/Navbar'
import { Footer } from '@/components/layout/Footer'
import { GlowOrb } from '@/components/effects/GlowOrb'
import { Hero } from '@/components/sections/Hero'
import { Stats } from '@/components/sections/Stats'
import { Features } from '@/components/sections/Features'
import { RaidExplainer } from '@/components/sections/RaidExplainer'
import { BanFeed } from '@/components/sections/BanFeed'
import { Dashboard } from '@/components/sections/Dashboard'
import { ClipManager } from '@/components/sections/ClipManager'
import { Community } from '@/components/sections/Community'
import { Security } from '@/components/sections/Security'
import { CTA } from '@/components/sections/CTA'
import { SiteChatbot } from '@/components/layout/SiteChatbot'

export default function App() {
  return (
    <>
      <GlowOrb />
      <Navbar />
      <main>
        <Hero />
        <RaidExplainer />
        <BanFeed />
        <Stats />
        <Dashboard />
        <Features />
        <ClipManager />
        <Community />
        <Security />
        <CTA />
      </main>
      <Footer />
      <SiteChatbot />
    </>
  )
}
