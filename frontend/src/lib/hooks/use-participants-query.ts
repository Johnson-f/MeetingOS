import { useQuery } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";
import { queryKeys } from "@/lib/service";

export function useParticipantsQuery(meetingId: string) {
  const client = useBackendClient();
  return useQuery({
    queryKey: queryKeys.participants(meetingId),
    queryFn: () => client.getParticipants(meetingId),
    enabled: Boolean(meetingId),
  });
}
