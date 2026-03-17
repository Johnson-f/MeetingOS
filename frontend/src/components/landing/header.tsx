"use client"

import { SignInButton, SignUpButton, Show, UserButton } from "@clerk/nextjs"
import { Button } from "@/components/ui/button"

export function LandingHeader() {
  return (
    <header className="fixed top-4 left-1/2 z-50 -translate-x-1/2">
      <nav className="flex items-center gap-6 rounded-full border border-foreground/10 bg-background/80 px-4 py-2 shadow-lg backdrop-blur-xl">
        <span className="text-sm font-semibold tracking-tight pl-2">Capsule</span>
        <Show when="signed-out">
          <div className="flex items-center gap-2">
            <SignInButton>
              <Button variant="ghost" size="sm">
                Sign In
              </Button>
            </SignInButton>
            <SignUpButton>
              <Button variant="default" size="sm">
                Sign Up
              </Button>
            </SignUpButton>
          </div>
        </Show>
        <Show when="signed-in">
          <div className="flex items-center gap-3">
            <Button variant="ghost" size="sm" render={<a href="/dashboard" />}>
              Dashboard
            </Button>
            <UserButton />
          </div>
        </Show>
      </nav>
    </header>
  )
}
