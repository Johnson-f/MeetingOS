import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useBackendClient } from "./use-backend-client";

export function useShareMutation(meetingId: string) {
  const client = useBackendClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (emails: string[]) => client.shareMeeting(meetingId, { emails }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["meetings"] });
    },
  });
}
