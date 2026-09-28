import type React from 'react'
import { Server } from 'lucide-react'

import { Box, Flex, Input, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'

interface McpServerSettingsProps {
  isLoading: boolean
  enabled: boolean
  port: string
  running: boolean
  onEnabledChange: (enabled: boolean) => void
  onPortChange: (port: string) => void
}

const McpServerSettings: React.FC<McpServerSettingsProps> = ({
  isLoading,
  enabled: mcpServerEnabled,
  port: mcpServerPort,
  running: mcpServerRunning,
  onEnabledChange,
  onPortChange,
}) => {
  const handlePortChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= 65535)) {
      onPortChange(value)
    }
  }

  return (
    <>
      {/* Left Column - MCP Server */}
      <Box
        bg='#161616'
        p={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        display='flex'
        flexDirection='column'
        height='100%'
      >
        <Flex align='center' gap={1.5} mb={1}>
          <Box as={Server} width='10px' height='10px' color='purple.400' />
          <Text fontSize='sm' fontWeight='500' color='white'>
            MCP Server
          </Text>
          <Box
            width='5px'
            height='5px'
            borderRadius='full'
            bg={mcpServerRunning ? 'green.400' : 'gray.500'}
            title={mcpServerRunning ? 'Running' : 'Stopped'}
          />
        </Flex>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3' flex='1'>
          Enable MCP server for AI assistants to manage port forwards via Model
          Context Protocol.
        </Text>
        <Box borderTop='1px solid rgba(255, 255, 255, 0.06)' mt={3} pt={3}>
          <Flex align='center' justify='flex-end' gap={2}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Enabled:
            </Text>
            <Checkbox
              checked={mcpServerEnabled}
              onCheckedChange={e => onEnabledChange(e.checked === true)}
              disabled={isLoading}
              size='sm'
            />
          </Flex>
        </Box>
      </Box>

      {/* Right Column - MCP Server Port */}
      <Box
        bg='#161616'
        p={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        display='flex'
        flexDirection='column'
        height='100%'
        opacity={mcpServerEnabled ? 1 : 0.5}
      >
        <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
          MCP Server Port
        </Text>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3' flex='1'>
          {mcpServerRunning
            ? `Running at http://127.0.0.1:${mcpServerPort}`
            : 'Server endpoint port'}
        </Text>
        <Box borderTop='1px solid rgba(255, 255, 255, 0.06)' mt={3} pt={3}>
          <Flex align='center' justify='flex-end' gap={2}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Port:
            </Text>
            <Input
              value={mcpServerPort}
              onChange={handlePortChange}
              placeholder='3000'
              size='xs'
              width='55px'
              height='22px'
              bg='#111111'
              border='1px solid rgba(255, 255, 255, 0.08)'
              _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
              _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
              color='white'
              _placeholder={{ color: 'whiteAlpha.500' }}
              disabled={isLoading || !mcpServerEnabled}
              textAlign='center'
              fontSize='xs'
            />
          </Flex>
        </Box>
      </Box>
    </>
  )
}

export default McpServerSettings
