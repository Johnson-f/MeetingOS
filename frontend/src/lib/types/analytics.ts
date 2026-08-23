export interface IntegrationPlaceholder {
  status: string;
  label: string;
}

export interface AnalyticsOverview {
  total_meetings: number;
  meetings_this_week_previous: number;
  meetings_this_week_upcoming: number;
  recorded_hours: number;
  integrations: IntegrationPlaceholder;
}
