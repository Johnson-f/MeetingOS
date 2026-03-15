import { SignInButton, SignUpButton, Show, UserButton } from "@clerk/nextjs";
import { Button } from "@/components/ui/button";

export default function Home() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-zinc-50 font-sans dark:bg-black">
      <main className="flex flex-col items-center gap-6">
        <h1 className="text-3xl font-semibold tracking-tight text-black dark:text-zinc-50">
          Meeting Bot
        </h1>
        <Show when="signed-out">
          <div className="flex gap-3">
            <SignInButton>
              <Button variant="default" size="lg">
                Sign In
              </Button>
            </SignInButton>
            <SignUpButton>
              <Button variant="outline" size="lg">
                Sign Up
              </Button>
            </SignUpButton>
          </div>
        </Show>
        <Show when="signed-in">
          <div className="flex flex-col items-center gap-4">
            <UserButton />
            <p className="text-zinc-600 dark:text-zinc-400">You are signed in.</p>
          </div>
        </Show>
      </main>
    </div>
  );
}
