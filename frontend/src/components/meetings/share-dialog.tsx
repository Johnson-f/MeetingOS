"use client"

import * as React from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { useShareMutation } from "@/lib/hooks/use-share-mutation"

function isValidEmail(email: string) {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)
}

export function ShareDialog({ meetingId }: { meetingId: string }) {
  const [open, setOpen] = React.useState(false)
  const [inputValue, setInputValue] = React.useState("")
  const [emails, setEmails] = React.useState<string[]>([])
  const [inputError, setInputError] = React.useState("")
  const [success, setSuccess] = React.useState(false)

  const shareMutation = useShareMutation(meetingId)

  function addEmail() {
    const trimmed = inputValue.trim()
    if (!trimmed) return
    if (!isValidEmail(trimmed)) {
      setInputError("Please enter a valid email address")
      return
    }
    if (emails.includes(trimmed)) {
      setInputError("Email already added")
      return
    }
    setEmails((prev) => [...prev, trimmed])
    setInputValue("")
    setInputError("")
  }

  function removeEmail(email: string) {
    setEmails((prev) => prev.filter((e) => e !== email))
  }

  function handleKeyDown(e: React.KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Enter") {
      e.preventDefault()
      addEmail()
    }
  }

  function handleSubmit() {
    if (emails.length === 0) return
    setSuccess(false)
    shareMutation.mutate(emails, {
      onSuccess: () => {
        setSuccess(true)
        setEmails([])
        setInputValue("")
      },
    })
  }

  function handleOpenChange(value: boolean) {
    setOpen(value)
    if (!value) {
      setEmails([])
      setInputValue("")
      setInputError("")
      setSuccess(false)
      shareMutation.reset()
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button variant="outline" size="sm" />}>
        Share
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Share Meeting</DialogTitle>
        </DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex gap-2">
            <Input
              placeholder="Enter email address"
              value={inputValue}
              onChange={(e) => {
                setInputValue(e.target.value)
                if (inputError) setInputError("")
              }}
              onKeyDown={handleKeyDown}
              aria-invalid={!!inputError}
            />
            <Button type="button" variant="outline" onClick={addEmail}>
              Add
            </Button>
          </div>
          {inputError && (
            <p className="text-xs text-destructive -mt-2">{inputError}</p>
          )}
          {emails.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {emails.map((email) => (
                <span
                  key={email}
                  className="inline-flex items-center gap-1 rounded-full bg-secondary px-2.5 py-0.5 text-xs font-medium"
                >
                  {email}
                  <button
                    type="button"
                    onClick={() => removeEmail(email)}
                    className="ml-0.5 rounded-full text-muted-foreground hover:text-foreground focus:outline-none"
                    aria-label={`Remove ${email}`}
                  >
                    &times;
                  </button>
                </span>
              ))}
            </div>
          )}
          {shareMutation.isError && (
            <p className="text-xs text-destructive">
              {(shareMutation.error as Error)?.message ?? "Failed to share meeting"}
            </p>
          )}
          {success && (
            <p className="text-xs text-green-600">Meeting shared successfully!</p>
          )}
          <Button
            onClick={handleSubmit}
            disabled={emails.length === 0 || shareMutation.isPending}
          >
            {shareMutation.isPending
              ? "Sending..."
              : `Send to ${emails.length} recipient${emails.length !== 1 ? "s" : ""}`}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
