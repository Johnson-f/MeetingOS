"use client"

import Image from "next/image"
import { SignUpButton, Show } from "@clerk/nextjs"
import { Button } from "@/components/ui/button"

export function Hero() {
  return (
    <section className="flex flex-col items-center px-6 pt-36 pb-20 text-center">
      <h1 className="max-w-3xl text-5xl font-semibold tracking-tight sm:text-6xl lg:text-7xl">
        Your meetings,{" "}
        <span className="text-muted-foreground">captured.</span>
      </h1>
      <p className="mt-6 max-w-xl text-lg text-muted-foreground">
        Capsule joins your calls, records everything, and turns conversations
        into searchable transcripts and smart notes — automatically.
      </p>
      <Show when="signed-out">
        <SignUpButton>
          <Button size="lg" className="mt-10 rounded-full px-8">
            Get Started
          </Button>
        </SignUpButton>
      </Show>
      <Show when="signed-in">
        <Button size="lg" className="mt-10 rounded-full px-8" render={<a href="/dashboard" />}>
          Go to Dashboard
        </Button>
      </Show>
      <div className="mt-16 w-full max-w-5xl">
        <Image
          src="/images/frame_generic_light.png"
          alt="Capsule dashboard"
          width={1920}
          height={1080}
          className="rounded-xl border border-foreground/5 shadow-2xl"
          priority
        />
      </div>
    </section>
  )
}
