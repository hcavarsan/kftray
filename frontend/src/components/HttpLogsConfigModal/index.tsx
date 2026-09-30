import { useState } from 'react'
import { FileText } from 'lucide-react'

import {
  Box,
  Dialog,
  Field,
  Flex,
  Grid,
  Input,
  Stack,
  Text,
} from '@chakra-ui/react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { AppDialog } from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

interface HttpLogsConfig {
  config_id: number
  enabled: boolean
  max_file_size: number
  retention_days: number
  auto_cleanup: boolean
}

interface HttpLogsConfigModalProps {
  configId: number
  onClose: () => void
  onSaved?: () => void
}

interface Draft {
  enabled: boolean
  auto_cleanup: boolean
  maxFileSizeMb: string
  retentionDays: string
}

type NumericField = 'maxFileSizeMb' | 'retentionDays'

type DraftErrors = Partial<Record<NumericField, string>>

const MB = 1024 * 1024

const toDraft = (config: HttpLogsConfig): Draft => ({
  enabled: config.enabled,
  auto_cleanup: config.auto_cleanup,
  maxFileSizeMb: String(config.max_file_size / MB),
  retentionDays: String(config.retention_days),
})

const parseInRange = (value: string, min: number, max: number) => {
  const trimmed = value.trim()
  const n = Number(trimmed)

  return trimmed !== '' && Number.isInteger(n) && n >= min && n <= max
    ? n
    : null
}

const formatFileSize = (bytes: number) => {
  if (bytes >= MB) {
    return `${(bytes / MB).toFixed(1)} MB`
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`
  }

  return `${bytes} bytes`
}

const cardProps = {
  bg: 'app.panel',
  p: 2.5,
  borderRadius: 'md',
  border: '1px solid',
  borderColor: 'app.border',
  height: 'fit-content',
}

const numberInputProps = {
  type: 'number',
  size: 'xs',
  width: '60px',
  height: '24px',
  bg: 'app.bg',
  border: '1px solid',
  borderColor: 'app.border',
  _hover: { borderColor: 'app.borderStrong' },
  _focus: { borderColor: 'blue.400', boxShadow: 'none' },
  _invalid: { borderColor: 'red.400' },
  color: 'white',
  _placeholder: { color: 'whiteAlpha.500' },
  textAlign: 'center',
  fontSize: 'xs',
} as const

export default function HttpLogsConfigModal({
  configId,
  onClose,
  onSaved,
}: HttpLogsConfigModalProps) {
  const queryClient = useQueryClient()
  const queryKey = ['http-logs-config', configId]
  const configQuery = useQuery({
    queryKey,
    queryFn: () =>
      invoke<HttpLogsConfig>('get_http_logs_config_cmd', { configId }),
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to load HTTP logs configuration',
        duration: 3000,
      },
    },
  })

  const [edits, setEdits] = useState<Partial<Draft>>({})
  const [errors, setErrors] = useState<DraftErrors>({})

  const draft: Draft | undefined = configQuery.data && {
    ...toDraft(configQuery.data),
    ...edits,
  }

  const update = (patch: Partial<Draft>) =>
    setEdits(prev => ({ ...prev, ...patch }))

  const updateNumber = (field: NumericField, value: string) => {
    update({ [field]: value })
    setErrors(prev => ({ ...prev, [field]: undefined }))
  }

  const saveMutation = useMutation({
    mutationFn: (config: HttpLogsConfig) =>
      invoke('update_http_logs_config_cmd', { config }),
    onSuccess: (_, config) => {
      queryClient.setQueryData(queryKey, config)
      toaster.success({
        title: 'Settings Saved',
        description: 'HTTP logs configuration has been saved successfully',
        duration: 3000,
      })
      onSaved?.()
      onClose()
    },
    onError: () => {
      toaster.error({
        title: 'Error',
        description: 'Failed to save HTTP logs configuration',
        duration: 3000,
      })
    },
  })

  const handleSave = () => {
    if (!configQuery.data || !draft) {
      return
    }
    const mb =
      edits.maxFileSizeMb === undefined
        ? configQuery.data.max_file_size / MB
        : parseInRange(edits.maxFileSizeMb, 1, 100)
    const days =
      edits.retentionDays === undefined
        ? configQuery.data.retention_days
        : parseInRange(edits.retentionDays, 1, 365)
    const nextErrors: DraftErrors = {
      ...(mb === null && {
        maxFileSizeMb: 'Enter a whole number from 1 to 100',
      }),
      ...(days === null && {
        retentionDays: 'Enter a whole number from 1 to 365',
      }),
    }

    setErrors(nextErrors)
    if (mb === null || days === null) {
      toaster.error({
        title: 'Invalid settings',
        description: 'Please fix highlighted fields before saving',
        duration: 3000,
      })

      return
    }
    saveMutation.mutate({
      config_id: configId,
      enabled: draft.enabled,
      auto_cleanup: draft.auto_cleanup,
      max_file_size: Math.round(mb * MB),
      retention_days: days,
    })
  }

  const fileSizeMb =
    edits.maxFileSizeMb === undefined
      ? null
      : parseInRange(edits.maxFileSizeMb, 1, 100)

  return (
    <AppDialog
      title={
        <Flex align='center' gap={2}>
          <Box as={FileText} width='14px' height='14px' color='blue.400' />
          <Text as='span' fontWeight='600' color='white'>
            HTTP Logs Configuration
          </Text>
        </Flex>
      }
      onClose={onClose}
      maxWidth='420px'
      contentProps={{
        boxShadow: 'dialog',
        css: {
          '&::-webkit-scrollbar': { display: 'none' },
          msOverflowStyle: 'none',
          scrollbarWidth: 'none',
        },
      }}
    >
      <Dialog.Body px={4} py={3}>
        {!draft ? (
          <Box py={6} textAlign='center'>
            <Text color={configQuery.isError ? 'red.300' : 'whiteAlpha.600'}>
              {configQuery.isError
                ? `Failed to load configuration: ${errorMessage(configQuery.error)}`
                : 'Loading configuration...'}
            </Text>
          </Box>
        ) : (
          <Stack gap={3}>
            <Grid templateColumns='1fr 1fr' gap={3}>
              <Box {...cardProps}>
                <Flex direction='column' gap={2}>
                  <Text fontSize='sm' fontWeight='500' color='white'>
                    Enable HTTP Logs
                  </Text>
                  <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                    Enable HTTP request/response logging for this configuration
                  </Text>
                  <Box alignSelf='flex-start'>
                    <Switch
                      aria-label='Enable HTTP Logs'
                      checked={draft.enabled}
                      onCheckedChange={details =>
                        update({ enabled: details.checked })
                      }
                      colorPalette='blue'
                    />
                  </Box>
                </Flex>
              </Box>

              <Box {...cardProps}>
                <Flex direction='column' gap={2}>
                  <Text fontSize='sm' fontWeight='500' color='white'>
                    Automatic Cleanup
                  </Text>
                  <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                    Automatically remove old log files based on retention period
                  </Text>
                  <Box alignSelf='flex-start'>
                    <Switch
                      aria-label='Automatic Cleanup'
                      checked={draft.auto_cleanup}
                      onCheckedChange={details =>
                        update({ auto_cleanup: details.checked })
                      }
                      colorPalette='blue'
                    />
                  </Box>
                </Flex>
              </Box>

              <Field.Root {...cardProps} invalid={!!errors.maxFileSizeMb}>
                <Flex direction='column' gap={2}>
                  <Field.Label fontSize='sm' fontWeight='500' color='white'>
                    Maximum File Size
                  </Field.Label>
                  <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                    Maximum file size before rotation. Current:{' '}
                    {formatFileSize(
                      fileSizeMb
                        ? fileSizeMb * MB
                        : (configQuery.data?.max_file_size ?? 0),
                    )}
                  </Text>
                  <Flex align='center' gap={1}>
                    <Input
                      {...numberInputProps}
                      value={draft.maxFileSizeMb}
                      onChange={e =>
                        updateNumber('maxFileSizeMb', e.target.value)
                      }
                      placeholder='10'
                      min={1}
                      max={100}
                    />
                    <Text fontSize='xs' color='whiteAlpha.600'>
                      MB
                    </Text>
                  </Flex>
                  <Field.ErrorText fontSize='xs'>
                    {errors.maxFileSizeMb}
                  </Field.ErrorText>
                </Flex>
              </Field.Root>

              <Field.Root {...cardProps} invalid={!!errors.retentionDays}>
                <Flex direction='column' gap={2}>
                  <Field.Label fontSize='sm' fontWeight='500' color='white'>
                    Retention Period
                  </Field.Label>
                  <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                    Days to keep log files before cleanup
                  </Text>
                  <Flex align='center' gap={1}>
                    <Input
                      {...numberInputProps}
                      value={draft.retentionDays}
                      onChange={e =>
                        updateNumber('retentionDays', e.target.value)
                      }
                      placeholder='7'
                      min={1}
                      max={365}
                    />
                    <Text fontSize='xs' color='whiteAlpha.600'>
                      days
                    </Text>
                  </Flex>
                  <Field.ErrorText fontSize='xs'>
                    {errors.retentionDays}
                  </Field.ErrorText>
                </Flex>
              </Field.Root>
            </Grid>
          </Stack>
        )}
      </Dialog.Body>

      <Dialog.Footer
        bg='app.panel'
        px={4}
        py={3}
        borderTop='1px solid'
        borderColor='app.border'
      >
        <Flex justify='flex-end' gap={2} width='100%'>
          <Button
            variant='ghost'
            size='xs'
            onClick={onClose}
            disabled={saveMutation.isPending}
            _hover={{ bg: 'whiteAlpha.100' }}
            color='whiteAlpha.700'
            height='28px'
            fontSize='xs'
          >
            Cancel
          </Button>
          <Button
            size='xs'
            onClick={handleSave}
            loading={saveMutation.isPending}
            loadingText='Saving...'
            disabled={!draft}
            bg='blue.500'
            color='white'
            _hover={{ bg: 'blue.600' }}
            _active={{ bg: 'blue.700' }}
            height='28px'
            fontSize='xs'
          >
            Save Settings
          </Button>
        </Flex>
      </Dialog.Footer>
    </AppDialog>
  )
}
