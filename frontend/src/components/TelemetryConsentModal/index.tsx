import { Stack, Text } from '@chakra-ui/react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { telemetryQuery } from '@/lib/telemetry'

// onDone runs after every answer, also after a failed save, so the caller
// can stop asking for the rest of the session.
export function TelemetryConsentModal({ onDone }: { onDone: () => void }) {
  const queryClient = useQueryClient()
  const choose = useMutation({
    mutationFn: (enabled: boolean) =>
      invoke('update_telemetry_enabled', { enabled }),
    onSettled: () => {
      onDone()

      return queryClient.invalidateQueries({
        queryKey: telemetryQuery.queryKey,
      })
    },
    meta: { errorToast: { title: 'Failed to save the crash report setting' } },
  })

  return (
    <AppDialog
      title='Crash reports'
      onClose={() => choose.mutate(false)}
      closeDisabled={choose.isPending}
      placement='center'
      maxWidth='400px'
    >
      <AppDialogBody>
        <Stack gap={2}>
          <Text fontSize='xs' color='fg.muted'>
            kftray can send a report when it crashes or hits an unexpected
            error. A report has the error type, where in the code it happened,
            the app version and the operating system.
          </Text>
          <Text fontSize='xs' color='fg.muted'>
            It never includes cluster names, namespaces, service names, aliases
            or kubeconfig files. Reports go to a server run by the kftray
            maintainer. You can change this later in Settings.
          </Text>
        </Stack>
      </AppDialogBody>
      <AppDialogFooter>
        <DialogCancelButton
          label='No thanks'
          onClick={() => choose.mutate(false)}
          disabled={choose.isPending}
        />
        <Button
          size='xs'
          height='28px'
          bg='accent.solid'
          color='fg'
          _hover={{ bg: 'accent.solidHover' }}
          _active={{ bg: 'accent.solidActive' }}
          loading={choose.isPending}
          onClick={() => choose.mutate(true)}
        >
          Send crash reports
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
