import { HugeiconsIcon } from "@hugeicons/react"
import { Video01Icon, FileSearchIcon, AiChat02Icon } from "@hugeicons/core-free-icons"

const features = [
  {
    icon: Video01Icon,
    title: "Auto-Recording",
    description:
      "A bot joins your Google Meet calls and records the entire session. No browser extensions, no manual setup.",
  },
  {
    icon: FileSearchIcon,
    title: "AI Transcription",
    description:
      "Every recording is transcribed with speaker labels and timestamps. Summarized into actionable notes you can reference later.",
  },
  {
    icon: AiChat02Icon,
    title: "Smart Search",
    description:
      "Ask questions about past meetings in natural language. Capsule finds the exact moment and context you need.",
  },
]

export function Features() {
  return (
    <section className="mx-auto max-w-5xl px-6 py-32">
      <h2 className="text-center text-3xl font-semibold tracking-tight sm:text-4xl">
        Everything happens after the call
      </h2>
      <p className="mt-4 text-center text-muted-foreground">
        Focus on the conversation. Capsule handles the rest.
      </p>
      <div className="mt-16 grid gap-8 sm:grid-cols-3">
        {features.map((feature) => (
          <div
            key={feature.title}
            className="flex flex-col items-start rounded-2xl border border-foreground/5 bg-muted/30 p-6"
          >
            <div className="flex size-10 items-center justify-center rounded-lg bg-primary text-primary-foreground">
              <HugeiconsIcon icon={feature.icon} strokeWidth={2} className="size-5" />
            </div>
            <h3 className="mt-4 text-base font-medium">{feature.title}</h3>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
              {feature.description}
            </p>
          </div>
        ))}
      </div>
    </section>
  )
}
