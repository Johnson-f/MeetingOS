import { LandingHeader } from "@/components/landing/header"
import { Hero } from "@/components/landing/hero"
import { Features } from "@/components/landing/features"
import { CTA } from "@/components/landing/cta"
import { Footer } from "@/components/landing/footer"

export default function Home() {
  return (
    <div className="min-h-screen bg-background font-sans">
      <LandingHeader />
      <Hero />
      <Features />
      <CTA />
      <Footer />
    </div>
  )
}
