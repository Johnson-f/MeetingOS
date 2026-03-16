import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"

export function IntegrationsView() {
  return (
    <div>
      <h1 className="mb-4 text-lg font-semibold">Integrations</h1>
      <Card>
        <CardHeader>
          <CardTitle>Integration coming soon</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="max-w-2xl text-sm text-muted-foreground">
            You&apos;d be able to connect to Notion, Jira, Slack, Discord so your
            meeting transcripts go anywhere you go.
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
