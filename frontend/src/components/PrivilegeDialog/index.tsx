import { useState } from 'react'
import { FileText, Network, ShieldCheck } from 'lucide-react'

import { Box, Flex, Stack, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { errorMessage } from '@/lib/errors'

import type { PrivilegedResource, PrivilegeNeed } from './types'

const RESOURCES: Record<
  PrivilegedResource,
  { icon: typeof Network; title: string; detail: string }
> = {
  hosts_file: {
    icon: FileText,
    title: 'Hosts file',
    detail: 'Domain aliases are written to the system hosts file.',
  },
  loopback_address: {
    icon: Network,
    title: 'Loopback address',
    detail: 'A separate 127.0.0.x is added to the loopback interface.',
  },
}

interface PrivilegeDialogProps {
  needs: PrivilegeNeed[]
  /** The backend's message when a start was already refused. */
  refusal: string | null
  isInstalling: boolean
  installError: unknown
  onInstall: () => void
  onContinue: () => void
  onCancel: () => void
  onSuppress: () => Promise<void>
}

/**
 * Shown before a start that would need admin access, or after one was
 * refused. Installing the helper takes one admin prompt and covers every
 * later start; continuing without it takes a prompt for each change on
 * this start.
 */
export function PrivilegeDialog({
  needs,
  refusal,
  isInstalling,
  installError,
  onInstall,
  onContinue,
  onCancel,
  onSuppress,
}: PrivilegeDialogProps) {
  const [suppress, setSuppress] = useState(false)
  const byResource = new Map<PrivilegedResource, string[]>()

  for (const need of needs) {
    const details = byResource.get(need.resource) ?? []

    if (!details.includes(need.detail)) {
      details.push(need.detail)
    }
    byResource.set(need.resource, details)
  }

  const continueWithout = async () => {
    if (suppress) {
      await onSuppress()
    }
    onContinue()
  }

  return (
    <AppDialog
      title={refusal ? 'Admin access was refused' : 'Admin access needed'}
      onClose={onCancel}
      closeDisabled={isInstalling}
      role='alertdialog'
      placement='center'
      maxWidth='420px'
    >
      <AppDialogBody>
        <Stack gap={2}>
          {refusal && (
            <Text
              fontSize='xs'
              color='danger.fg'
              bg='danger.subtle'
              border='1px solid'
              borderColor='danger.muted'
              borderRadius='md'
              px={2.5}
              py={2}
              title={refusal}
              lineClamp={3}
            >
              {refusal}
            </Text>
          )}
          <Flex layerStyle='card' p={3} gap={3} align='center'>
            <Flex
              align='center'
              justify='center'
              width='36px'
              height='36px'
              borderRadius='full'
              bg='accent.subtle'
              color='accent.fg'
              flexShrink={0}
            >
              <Box as={ShieldCheck} width='16px' height='16px' />
            </Flex>
            <Stack gap={0.5} flex='1' minWidth={0}>
              <Text fontSize='sm' fontWeight='500' color='fg'>
                Install kftray-helper
              </Text>
              <Text fontSize='xs' color='fg.subtle'>
                One admin prompt now. Later starts need none.
              </Text>
            </Stack>
          </Flex>

          {byResource.size > 0 && (
            <Box layerStyle='card' overflow='hidden'>
              {[...byResource].map(([resource, details], index) => {
                const { icon, title, detail } = RESOURCES[resource]

                return (
                  <Stack
                    key={resource}
                    gap={2}
                    p={3}
                    borderTop={index > 0 ? '1px solid' : undefined}
                    borderColor='border.subtle'
                  >
                    <Flex align='center' gap={3}>
                      <Flex
                        align='center'
                        justify='center'
                        width='28px'
                        height='28px'
                        borderRadius='md'
                        bg='bg.faint'
                        color='fg.muted'
                        flexShrink={0}
                      >
                        <Box as={icon} width='14px' height='14px' />
                      </Flex>
                      <Stack gap={0} flex='1' minWidth={0}>
                        <Text fontSize='xs' fontWeight='500' color='fg'>
                          {title}
                        </Text>
                        <Text fontSize='11px' color='fg.subtle'>
                          {detail}
                        </Text>
                      </Stack>
                    </Flex>
                    <Flex wrap='wrap' gap={1} pl='40px'>
                      {details.map(item => (
                        <Text
                          key={item}
                          fontSize='10px'
                          fontFamily='mono'
                          color='fg.muted'
                          bg='bg.canvas'
                          border='1px solid'
                          borderColor='border.subtle'
                          borderRadius='sm'
                          px={1.5}
                          py={0.5}
                        >
                          {item}
                        </Text>
                      ))}
                    </Flex>
                  </Stack>
                )
              })}
            </Box>
          )}

          {installError != null && !isInstalling && (
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
              {errorMessage(installError)}
            </Text>
          )}

          <Checkbox
            size='sm'
            checked={suppress}
            onCheckedChange={({ checked }) => setSuppress(checked === true)}
            disabled={isInstalling}
          >
            <Text fontSize='xs' color='fg.muted'>
              Don't ask again, always continue without the helper
            </Text>
          </Checkbox>
        </Stack>
      </AppDialogBody>

      <AppDialogFooter justify='space-between'>
        <DialogCancelButton onClick={onCancel} disabled={isInstalling} />
        <Flex gap={2}>
          <Button
            size='xs'
            variant='ghost'
            height='28px'
            color='fg.muted'
            _hover={{ bg: 'bg.faint' }}
            onClick={continueWithout}
            disabled={isInstalling}
          >
            {refusal ? 'Retry without' : 'Continue without'}
          </Button>
          <Button
            size='xs'
            onClick={onInstall}
            loading={isInstalling}
            loadingText='Installing...'
            bg='accent.solid'
            color='fg'
            _hover={{ bg: 'accent.solidHover' }}
            _active={{ bg: 'accent.solidActive' }}
            height='28px'
            fontSize='xs'
          >
            Install helper
          </Button>
        </Flex>
      </AppDialogFooter>
    </AppDialog>
  )
}
