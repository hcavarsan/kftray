import { Component, type ReactNode } from 'react'

import { Flex, Stack, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { captureRenderError } from '@/lib/telemetry'

interface ErrorBoundaryProps {
  children: ReactNode
}

interface ErrorBoundaryState {
  failed: boolean
}

export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { failed: false }

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true }
  }

  componentDidCatch(error: unknown) {
    captureRenderError(error)
  }

  render() {
    if (!this.state.failed) {
      return this.props.children
    }
    return (
      <Flex height='100vh' align='center' justify='center' bg='bg.canvas'>
        <Stack gap={3} align='center' maxWidth='320px' textAlign='center'>
          <Text fontSize='sm' fontWeight='500' color='fg'>
            kftray ran into a problem
          </Text>
          <Text fontSize='xs' color='fg.subtle'>
            Reload the window to continue. With crash reports on, a report was
            sent.
          </Text>
          <Button
            size='sm'
            height='28px'
            bg='accent.solid'
            color='fg'
            _hover={{ bg: 'accent.solidHover' }}
            _active={{ bg: 'accent.solidActive' }}
            onClick={() => window.location.reload()}
          >
            Reload
          </Button>
        </Stack>
      </Flex>
    )
  }
}
