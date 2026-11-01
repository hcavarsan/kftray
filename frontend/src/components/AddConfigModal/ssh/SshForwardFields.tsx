import { useState } from 'react'
import { Check, Minus, X } from 'lucide-react'

import { Box, Flex, Grid, Stack, Text } from '@chakra-ui/react'
import { useMutation } from '@tanstack/react-query'

import { Button } from '@/components/ui/button'
import { selectStyles } from '@/components/ui/select-styles'
import { invoke } from '@/lib/tauri'
import type { SshTestStep, StoredConfig } from '@/types'

import { Field, FieldSelect, LocalAddressField, TextField } from '../Field'
import type { ConfigDraft, StringOption } from '../types'
import { toConfig, trimConfigValues, validateDraft } from '../utils'

import { SSH_PROVIDERS, sshProviderUi } from './providers'

const STATUS_ICONS = {
  failed: { Icon: X, color: 'danger.fg' },
  ok: { Icon: Check, color: 'success.fg' },
  skipped: { Icon: Minus, color: 'fg.subtle' },
} as const

const providerOptions: StringOption[] = Object.entries(SSH_PROVIDERS).map(
  ([value, { label }]) => ({ label, value }),
)

const STEP_LABELS: Record<Exclude<SshTestStep['step'], 'channel'>, string> = {
  route: 'Resolve bastion',
  transport: 'Open tunnel',
  handshake: 'SSH handshake and host key',
  auth: 'Authenticate',
}

// Unprivileged ports only: below 1024 the backend picks a free port instead.
const mirroredLocalPort = (remotePort: string) => {
  const port = Number(remotePort)

  return Number.isInteger(port) && port >= 1024 && port <= 65535
    ? remotePort
    : ''
}

function TestStepRow({ label, step }: { label: string; step: SshTestStep }) {
  const { Icon, color } = STATUS_ICONS[step.status]

  return (
    <Stack gap={0.5}>
      <Flex align='center' gap={1.5}>
        <Box
          as={Icon}
          aria-label={step.status}
          color={color}
          flexShrink={0}
          height='12px'
          width='12px'
        />
        <Text
          color={step.status === 'skipped' ? 'fg.subtle' : 'fg'}
          fontSize='xs'
        >
          {label}
        </Text>
      </Flex>
      {step.message && (
        <Text
          color={step.status === 'failed' ? 'danger.fg' : 'fg.muted'}
          fontSize='xs'
          pl={4.5}
          wordBreak='break-word'
        >
          {step.message}
        </Text>
      )}
    </Stack>
  )
}

interface SshForwardFieldsProps {
  draft: ConfigDraft
  errors: Record<string, string>
  initialConfig: StoredConfig | null
  onUpdate: (update: Partial<ConfigDraft>) => void
}

export function SshForwardFields({
  draft,
  errors,
  initialConfig,
  onUpdate,
}: SshForwardFieldsProps) {
  const [localPortEdited, setLocalPortEdited] = useState(
    Boolean(draft.local_port),
  )
  const provider = draft.ssh_provider ?? SSH_PROVIDERS['gcp-bastion'].empty()
  const ui = sshProviderUi(provider.type)
  const providerErrors = Object.fromEntries(
    Object.entries(ui?.validate(provider) ?? {}).filter(([field]) =>
      provider[field as keyof typeof provider]?.toString().trim(),
    ),
  )
  const testable = Object.keys(validateDraft(draft)).every(
    field => field === 'tags',
  )
  const destination = `${draft.remote_address?.trim() ?? ''}:${draft.remote_port ?? ''}`

  const testMutation = useMutation({
    mutationFn: (config: StoredConfig) =>
      invoke<SshTestStep[]>('test_ssh_forward_cmd', { config }),
    meta: { errorToast: { title: 'SSH connection test failed' } },
  })
  const forgetMutation = useMutation({
    mutationFn: (config: StoredConfig) =>
      invoke('forget_ssh_host_key_cmd', { config }),
    onSuccess: (_, config) => testMutation.mutate(config),
    meta: { errorToast: { title: 'Failed to forget the host key' } },
  })

  // A result describes the form as it was tested.
  const update = (change: Partial<ConfigDraft>) => {
    testMutation.reset()
    onUpdate(change)
  }
  const currentConfig = toConfig(trimConfigValues(draft), initialConfig)
  const hostKeyChange = testMutation.data?.find(
    ({ step, status }) => step === 'handshake' && status === 'failed',
  )?.host_key

  return (
    <>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <Field data-testid='config-ssh-provider' label='Provider *'>
          <FieldSelect<StringOption>
            onChange={option => {
              const next = sshProviderUi(option?.value ?? '')

              if (next && option?.value !== provider.type) {
                update({ ssh_provider: next.empty() })
              }
            }}
            options={providerOptions}
            styles={selectStyles<StringOption>()}
            value={
              providerOptions.find(option => option.value === provider.type) ??
              null
            }
          />
        </Field>
      </Grid>
      {ui ? (
        <ui.Form
          errors={providerErrors}
          onChange={ssh_provider => update({ ssh_provider })}
          provider={provider}
        />
      ) : (
        <Text color='warning.fg' fontSize='xs'>
          This SSH provider needs a newer kftray
        </Text>
      )}
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <TextField
          data-testid='config-ssh-remote-address'
          error={errors.remote_address}
          label='Remote Host *'
          name='remote_address'
          onChange={remote_address => update({ remote_address })}
          placeholder='e.g., db.internal.example.com'
          value={draft.remote_address ?? ''}
        />
        <TextField
          data-testid='config-ssh-remote-port'
          error={errors.remote_port}
          label='Remote Port *'
          name='remote_port'
          onChange={remote_port =>
            update(
              localPortEdited
                ? { remote_port }
                : { local_port: mirroredLocalPort(remote_port), remote_port },
            )
          }
          value={draft.remote_port ?? ''}
        />
      </Grid>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <TextField
          data-testid='config-ssh-local-port'
          error={errors.local_port}
          label='Local Port'
          name='local_port'
          onChange={local_port => {
            setLocalPortEdited(true)
            update({ local_port })
          }}
          type='number'
          value={draft.local_port ?? ''}
        />
        <LocalAddressField draft={draft} onUpdate={update} />
      </Grid>
      <Stack gap={2}>
        <Box>
          <Button
            data-testid='config-ssh-test'
            disabled={!testable}
            loading={testMutation.isPending}
            loadingText='Testing...'
            onClick={() => testMutation.mutate(currentConfig)}
            size='2xs'
            type='button'
            variant='outline'
          >
            Test connection
          </Button>
        </Box>
        {testMutation.data && (
          <Stack data-testid='config-ssh-test-result' gap={1}>
            {testMutation.data.map(step => (
              <TestStepRow
                key={step.step}
                label={
                  step.step === 'channel'
                    ? `Reach ${destination}`
                    : STEP_LABELS[step.step]
                }
                step={step}
              />
            ))}
            {hostKeyChange && (
              <Stack gap={1}>
                <Text color='fg.muted' fontSize='xs' wordBreak='break-all'>
                  Known: {hostKeyChange.known}
                </Text>
                <Text color='fg.muted' fontSize='xs' wordBreak='break-all'>
                  Presented: {hostKeyChange.presented}
                </Text>
                <Box>
                  <Button
                    data-testid='config-ssh-forget-host-key'
                    loading={forgetMutation.isPending}
                    loadingText='Forgetting...'
                    onClick={() => forgetMutation.mutate(currentConfig)}
                    size='2xs'
                    type='button'
                    variant='outline'
                  >
                    Forget host key
                  </Button>
                </Box>
              </Stack>
            )}
          </Stack>
        )}
      </Stack>
    </>
  )
}
