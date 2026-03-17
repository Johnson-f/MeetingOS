export function Footer() {
  return (
    <footer className="border-t border-foreground/5 py-8 text-center text-sm text-muted-foreground">
      &copy; {new Date().getFullYear()} Capsule. All rights reserved.
    </footer>
  )
}
