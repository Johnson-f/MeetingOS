"use client"

import { SignUpButton, Show } from "@clerk/nextjs"
import { Button } from "@/components/ui/button"

export function CTA() {
  return (
    <section className="flex flex-col items-center px-6 py-32 text-center">
      <h2 className="max-w-2xl text-3xl font-semibold tracking-tight sm:text-4xl">
        Stop losing what was said in meetings
      </h2>
      <p className="mt-4 max-w-lg text-muted-foreground">
        Set up in under a minute. No credit card required.
      </p>
      <Show when="signed-out">
        <SignUpButton>
          <Button size="lg" className="mt-8 rounded-full px-8">
            Get Started for Free
          </Button>
        </SignUpButton>
      </Show>
      <Show when="signed-in">
        <Button size="lg" className="mt-8 rounded-full px-8" render={<a href="/dashboard" />}>
          Go to Dashboard
        </Button>
      </Show>
    </section>
  )
}
