"use client"

import { usePathname } from "next/navigation"
import { Separator } from "@/components/ui/separator"
import { SidebarTrigger } from "@/components/ui/sidebar"
import { ChatTrigger } from "@/components/chat"

const pageTitles: Record<string, string> = {
  "/dashboard": "Dashboard",
  "/dashboard/meetings": "Meetings",
  "/dashboard/integrations": "Integration",
  "/dashboard/schedule": "Schedule",
}

export function SiteHeader() {
  const pathname = usePathname()

  const title = pageTitles[pathname] ?? "Dashboard"

  return (
    <header className="flex h-(--header-height) shrink-0 items-center gap-2 border-b transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-(--header-height)">
      <div className="flex w-full items-center justify-between gap-1 px-4 lg:gap-2 lg:px-6">
        <div className="flex items-center gap-1 lg:gap-2">
          <SidebarTrigger className="-ml-1" />
          <Separator
            orientation="vertical"
            className="mx-2 h-4 data-vertical:self-auto"
          />
          <h1 className="text-base font-medium">{title}</h1>
        </div>
        <ChatTrigger />
      </div>
    </header>
  )
}
