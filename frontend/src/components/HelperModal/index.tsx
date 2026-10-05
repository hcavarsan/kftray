import { useState } from 'react'
import { RefreshCw } from 'lucide-react'

import { Box, Flex, Spinner, Stack, Text } from '@chakra-ui/react'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  ConfirmDialog,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { Tooltip } from '@/components/ui/tooltip'
import { errorMessage } from '@/lib/errors'

import { FeatureList, type HelperPhase, StatusHeader } from './HelperSections'
import type { HelperStatus } from './types'
import { HELPER_STATUS_KEY, useHelperMutations } from './useHelper'

export function HelperModal({ onClose }: { onClose: () => void }) {
  const [confirmUninstall, setConfirmUninstall] = useState(false)
  const statusQuery = useQuery({
    queryKey: HELPER_STATUS_KEY,
    queryFn: () => invoke<HelperStatus>('get_helper_status'),
    staleTime: 0,
    retry: false,
  })
  const { installMutation, uninstallMutation, refreshStatus } =
    useHelperMutations()

  const status = statusQuery.data
  const busy = installMutation.isPending || uninstallMutation.isPending
  const actionError = busy
    ? null
    : (installMutation.error ?? uninstallMutation.error)

  let phase: HelperPhase = 'stopped'

  if (installMutation.isPending) {
    phase = 'installing'
  } else if (uninstallMutation.isPending) {
    phase = 'removing'
  } else if (status?.running) {
    phase = 'running'
  } else if (status?.error) {
    phase = 'unresponsive'
  }

  return (
    <>
      <AppDialog
        title='kftray-helper'
        onClose={onClose}
        closeDisabled={busy}
        maxWidth='420px'
      >
        <AppDialogBody>
          {!status ? (
            <Flex align='center' justify='center' minHeight='160px'>
              {statusQuery.error ? (
                <Text fontSize='xs' color='danger.fg'>
                  {errorMessage(statusQuery.error)}
                </Text>
              ) : (
                <Spinner size='sm' color='fg.subtle' />
              )}
            </Flex>
          ) : (
            <Stack gap={2}>
              <StatusHeader
                phase={phase}
                error={status.error}
                action={
                  <Tooltip content='Refresh'>
                    <Button
                      aria-label='Refresh'
                      size='xs'
                      variant='ghost'
                      onClick={refreshStatus}
                      disabled={statusQuery.isFetching || busy}
                      height='24px'
                      px={1.5}
                      color='fg.subtle'
                      _hover={{ bg: 'bg.faint', color: 'fg' }}
                    >
                      <Box
                        as={RefreshCw}
                        width='12px'
                        height='12px'
                        animation={
                          statusQuery.isFetching
                            ? 'spin 1s linear infinite'
                            : undefined
                        }
                      />
                    </Button>
                  </Tooltip>
                }
              />

              {actionError && (
                <Text
                  fontSize='xs'
                  color='danger.fg'
                  bg='danger.subtle'
                  border='1px solid'
                  borderColor='danger.muted'
                  borderRadius='md'
                  px={2.5}
                  py={2}
                >
                  {errorMessage(actionError)}
                </Text>
              )}

              <FeatureList
                addresses={
                  status.running
                    ? (status.addresses?.map(({ address, service }) => ({
                        key: address,
                        label: address,
                        hint: service || 'unnamed service',
                      })) ?? null)
                    : undefined
                }
                hosts={
                  status.running
                    ? (status.host_entries?.map(({ id, hostname, ip }) => ({
                        key: `${id}-${hostname}`,
                        label: hostname,
                        hint: ip,
                      })) ?? null)
                    : undefined
                }
              />
            </Stack>
          )}
        </AppDialogBody>

        <AppDialogFooter justify='space-between'>
          <Box>
            {status && (
              <Button
                size='xs'
                variant='ghost'
                height='28px'
                color='fg.subtle'
                _hover={{ bg: 'bg.faint', color: 'danger.fg' }}
                onClick={() => setConfirmUninstall(true)}
                disabled={busy}
              >
                Uninstall
              </Button>
            )}
          </Box>

          <Flex gap={2}>
            <DialogCancelButton
              label='Close'
              onClick={onClose}
              disabled={busy}
            />
            {status && !status.running && (
              <Button
                size='xs'
                onClick={() => installMutation.mutate()}
                loading={installMutation.isPending}
                loadingText='Installing...'
                disabled={uninstallMutation.isPending}
                bg='accent.solid'
                color='fg'
                _hover={{ bg: 'accent.solidHover' }}
                _active={{ bg: 'accent.solidActive' }}
                height='28px'
                fontSize='xs'
              >
                Install
              </Button>
            )}
          </Flex>
        </AppDialogFooter>
      </AppDialog>

      {confirmUninstall && (
        <ConfirmDialog
          title='Uninstall kftray-helper?'
          description='Loopback addresses and hosts aliases will need admin access again.'
          isPending={uninstallMutation.isPending}
          confirmLabel='Uninstall'
          pendingLabel='Uninstalling...'
          onConfirm={() =>
            uninstallMutation.mutate(undefined, {
              onSettled: () => setConfirmUninstall(false),
            })
          }
          onClose={() => setConfirmUninstall(false)}
        />
      )}
    </>
  )
}
