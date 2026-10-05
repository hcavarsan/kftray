import type { ComponentType, ReactNode } from 'react'
import { FileText, Network, ShieldCheck, ShieldOff } from 'lucide-react'

import { Box, Flex, Spinner, Stack, Text } from '@chakra-ui/react'

export type HelperPhase =
  | 'running'
  | 'stopped'
  | 'unresponsive'
  | 'installing'
  | 'removing'

const PHASES: Record<
  HelperPhase,
  { title: string; detail: string; color: string; bg: string }
> = {
  running: {
    title: 'Running',
    detail: 'Handles admin-only network changes.',
    color: 'success.fg',
    bg: 'success.subtle',
  },
  stopped: {
    title: 'Not running',
    detail: 'Install once to skip admin prompts.',
    color: 'fg.subtle',
    bg: 'bg.faint',
  },
  unresponsive: {
    title: 'Not responding',
    detail: 'The helper socket exists but did not answer.',
    color: 'danger.fg',
    bg: 'danger.subtle',
  },
  installing: {
    title: 'Installing',
    detail: 'Approve the admin prompt to continue.',
    color: 'accent.fg',
    bg: 'accent.subtle',
  },
  removing: {
    title: 'Removing',
    detail: 'Approve the admin prompt if one appears.',
    color: 'accent.fg',
    bg: 'accent.subtle',
  },
}

export function StatusHeader({
  phase,
  error,
  action,
}: {
  phase: HelperPhase
  error: string | null
  action: ReactNode
}) {
  const { title, detail, color, bg } = PHASES[phase]
  const pending = phase === 'installing' || phase === 'removing'

  return (
    <Flex layerStyle='card' p={3} gap={3} align='center'>
      <Flex
        align='center'
        justify='center'
        width='36px'
        height='36px'
        borderRadius='full'
        bg={bg}
        color={color}
        flexShrink={0}
      >
        {pending ? (
          <Spinner size='xs' />
        ) : (
          <Box
            as={phase === 'running' ? ShieldCheck : ShieldOff}
            width='16px'
            height='16px'
          />
        )}
      </Flex>
      <Stack gap={0.5} flex='1' minWidth={0}>
        <Text fontSize='sm' fontWeight='500' color='fg'>
          {title}
        </Text>
        <Text
          fontSize='xs'
          color='fg.subtle'
          truncate
          title={error ?? undefined}
        >
          {phase === 'unresponsive' && error ? error : detail}
        </Text>
      </Stack>
      {action}
    </Flex>
  )
}

interface FeatureItem {
  key: string
  label: string
  hint: string
}

function FeatureRow({
  icon,
  title,
  detail,
  items,
  divider = false,
}: {
  icon: ComponentType<{ className?: string }>
  title: string
  detail: string
  items: FeatureItem[] | null | undefined
  divider?: boolean
}) {
  return (
    <Stack
      gap={2}
      p={3}
      borderTop={divider ? '1px solid' : undefined}
      borderColor='border.subtle'
    >
      <Flex align='center' gap={3}>
        <Flex
          align='center'
          justify='center'
          width='28px'
          height='28px'
          borderRadius='md'
          bg='accent.subtle'
          color='accent.fg'
          flexShrink={0}
        >
          <Box as={icon} width='14px' height='14px' />
        </Flex>
        <Stack gap={0} flex='1' minWidth={0}>
          <Text fontSize='xs' fontWeight='500' color='fg'>
            {title}
          </Text>
          <Text fontSize='11px' color='fg.subtle' truncate>
            {detail}
          </Text>
        </Stack>
        {items !== undefined && (
          <Text
            fontSize='10px'
            color='fg.muted'
            bg='bg.faint'
            px={1.5}
            py={0.5}
            borderRadius='full'
            title={
              items === null ? 'Could not read from the helper' : undefined
            }
          >
            {items === null ? '?' : items.length}
          </Text>
        )}
      </Flex>
      {items && items.length > 0 && (
        <Flex wrap='wrap' gap={1} pl='40px'>
          {items.map(item => (
            <Text
              key={item.key}
              title={item.hint}
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
              {item.label}
            </Text>
          ))}
        </Flex>
      )}
    </Stack>
  )
}

/**
 * Item lists are `undefined` while the helper is not running, so the rows
 * only show counts for what a live helper reported.
 */
export function FeatureList({
  addresses,
  hosts,
}: {
  addresses: FeatureItem[] | null | undefined
  hosts: FeatureItem[] | null | undefined
}) {
  return (
    <Box layerStyle='card' overflow='hidden'>
      <FeatureRow
        icon={Network}
        title='Loopback addresses'
        detail='A separate 127.0.0.x for each forward'
        items={addresses}
      />
      <FeatureRow
        icon={FileText}
        title='Hosts aliases'
        detail='Reach forwarded services by name'
        items={hosts}
        divider
      />
    </Box>
  )
}
