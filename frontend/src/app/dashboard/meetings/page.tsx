import { AppSidebar } from "@/components/app-sidebar";
import { MeetingsView } from "@/components/meetings/meetings-view";
import { RealtimeProvider } from "@/components/realtime-provider";
import { SiteHeader } from "@/components/site-header";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { ChatProvider } from "@/components/chat";

export default function MeetingsPage() {
  return (
    <RealtimeProvider>
      <ChatProvider>
      <SidebarProvider
        style={
          {
            "--sidebar-width": "calc(var(--spacing) * 72)",
            "--header-height": "calc(var(--spacing) * 12)",
          } as React.CSSProperties
        }
      >
        <AppSidebar variant="inset" />
        <SidebarInset>
          <SiteHeader />
          <div className="flex flex-1 flex-col">
            <div className="flex flex-1 flex-col gap-4 py-4 px-4 md:gap-6 md:py-6 lg:px-6">
              <MeetingsView />
            </div>
          </div>
        </SidebarInset>
      </SidebarProvider>
      </ChatProvider>
    </RealtimeProvider>
  );
}
