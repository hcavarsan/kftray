import { useState } from 'react'

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
import { Switch } from '@/components/ui/switch'
import { type Consent, performanceQuery, telemetryQuery } from '@/lib/telemetry'

type Choices = Record<Consent, boolean>

const CONSENTS: Record<
  Consent,
  { label: string; description: string; command: string }
> = {
  crashReports: {
    label: 'Crash reports',
    description: 'Send a report when kftray crashes.',
    command: 'update_telemetry_enabled',
  },
  performance: {
    label: 'Performance data',
    description: 'Send how long port forwards take to start and stop.',
    command: 'update_performance_enabled',
  },
}

const DECLINED: Choices = { crashReports: false, performance: false }

// onDone runs after every answer, also after a failed save, so the caller
// can stop asking for the rest of the session.
export function TelemetryConsentModal({
  consents,
  onDone,
}: {
  consents: Consent[]
  onDone: () => void
}) {
  const queryClient = useQueryClient()
  const [choices, setChoices] = useState<Choices>(DECLINED)
  const choose = useMutation({
    mutationFn: (answer: Choices) =>
      Promise.all(
        consents.map(consent =>
          invoke(CONSENTS[consent].command, { enabled: answer[consent] }),
        ),
      ),
    onSettled: () => {
      onDone()

      return Promise.all(
        [telemetryQuery, performanceQuery].map(query =>
          queryClient.invalidateQueries({ queryKey: query.queryKey }),
        ),
      )
    },
    meta: { errorToast: { title: 'Failed to save the diagnostics setting' } },
  })

  return (
    <AppDialog
      data-testid='consent-dialog'
      title='Help improve kftray'
      onClose={() => choose.mutate(DECLINED)}
      closeDisabled={choose.isPending}
      placement='center'
      maxWidth='400px'
    >
      <AppDialogBody>
        <Stack gap={3}>
          {consents.map(consent => (
            <Stack key={consent} gap={1}>
              <Switch
                size='sm'
                checked={choices[consent]}
                onCheckedChange={details =>
                  setChoices(prev => ({ ...prev, [consent]: details.checked }))
                }
                disabled={choose.isPending}
              >
                <Text fontSize='xs' fontWeight='500'>
                  {CONSENTS[consent].label}
                </Text>
              </Switch>
              <Text fontSize='xs' color='fg.muted'>
                {CONSENTS[consent].description}
              </Text>
            </Stack>
          ))}
          <Text fontSize='xs' color='fg.muted'>
            No cluster, namespace or service names are ever sent. You can change
            this in Settings.
          </Text>
        </Stack>
      </AppDialogBody>
      <AppDialogFooter>
        <DialogCancelButton
          data-testid='consent-decline'
          label='No thanks'
          onClick={() => choose.mutate(DECLINED)}
          disabled={choose.isPending}
        />
        <Button
          data-testid='consent-allow'
          size='xs'
          height='28px'
          bg='accent.solid'
          color='fg'
          _hover={{ bg: 'accent.solidHover' }}
          _active={{ bg: 'accent.solidActive' }}
          loading={choose.isPending}
          onClick={() => choose.mutate(choices)}
        >
          Save
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
