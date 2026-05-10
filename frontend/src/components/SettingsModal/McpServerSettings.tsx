import React, { useEffect, useState } from 'react'
import { Server } from 'lucide-react'

import { Box, Flex, Input, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'

interface McpServerSettingsProps {
  isLoading: boolean
}

interface McpStatus {
  enabled: string
  port: string
  running: string
}

const McpServerSettings: React.FC<McpServerSettingsProps> = ({ isLoading }) => {
  const [mcpServerEnabled, setMcpServerEnabled] = useState<boolean>(false)
  const [mcpServerPort, setMcpServerPort] = useState<string>('3000')
  const [mcpServerRunning, setMcpServerRunning] = useState<boolean>(false)
  const [isMcpToggling, setIsMcpToggling] = useState(false)

  useEffect(() => {
    loadMcpStatus()
  }, [])

  const loadMcpStatus = async () => {
    try {
      const status = await invoke<McpStatus>('get_mcp_server_status')

      setMcpServerEnabled(status.enabled === 'true')
      setMcpServerPort(status.port || '3000')
      setMcpServerRunning(status.running === 'true')
    } catch (error) {
      console.error('Error loading MCP status:', error)
      setMcpServerEnabled(false)
      setMcpServerPort('3000')
      setMcpServerRunning(false)
    }
  }

  const toggleMcpServer = async (enabled: boolean) => {
    try {
      setIsMcpToggling(true)
      await invoke('update_mcp_server_enabled', { enabled })
      setMcpServerEnabled(enabled)
      await loadMcpStatus()

      toaster.success({
        title: enabled ? 'MCP Server Started' : 'MCP Server Stopped',
        description: enabled
          ? `Server running at http://127.0.0.1:${mcpServerPort}`
          : 'MCP server has been stopped',
        duration: 3000,
      })
    } catch (error) {
      console.error('Error toggling MCP server:', error)
      toaster.error({
        title: 'Error',
        description: `Failed to ${enabled ? 'start' : 'stop'} MCP server: ${error}`,
        duration: 4000,
      })
    } finally {
      setIsMcpToggling(false)
    }
  }

  const handleMcpPortChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= 65535)) {
      setMcpServerPort(value)
    }
  }

  const saveMcpPort = async () => {
    const portValue = parseInt(mcpServerPort, 10)

    if (isNaN(portValue) || portValue < 1 || portValue > 65535) {
      toaster.error({
        title: 'Invalid Port',
        description: 'Port must be between 1 and 65535',
        duration: 3000,
      })

      return
    }

    try {
      await invoke('update_mcp_server_port', { port: portValue })
      await loadMcpStatus()

      toaster.success({
        title: 'Port Updated',
        description: mcpServerRunning
          ? `Server restarted on port ${portValue}`
          : `Port set to ${portValue}`,
        duration: 3000,
      })
    } catch (error) {
      console.error('Error updating MCP port:', error)
      toaster.error({
        title: 'Error',
        description: `Failed to update port: ${error}`,
        duration: 4000,
      })
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
          <Box
            as={Server}
            width='10px'
            height='10px'
            color='purple.400'
          />
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
        <Text
          fontSize='xs'
          color='whiteAlpha.600'
          lineHeight='1.3'
          flex='1'
        >
          Enable MCP server for AI assistants to manage port forwards via Model Context Protocol.
        </Text>
        <Box
          borderTop='1px solid rgba(255, 255, 255, 0.06)'
          mt={3}
          pt={3}
        >
          <Flex align='center' justify='flex-end' gap={2}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Enabled:
            </Text>
            <Checkbox
              checked={mcpServerEnabled}
              onCheckedChange={e => toggleMcpServer(e.checked === true)}
              disabled={isLoading || isMcpToggling}
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
        <Text
          fontSize='xs'
          color='whiteAlpha.600'
          lineHeight='1.3'
          flex='1'
        >
          {mcpServerRunning
            ? `Running at http://127.0.0.1:${mcpServerPort}`
            : 'Server endpoint port'}
        </Text>
        <Box
          borderTop='1px solid rgba(255, 255, 255, 0.06)'
          mt={3}
          pt={3}
        >
          <Flex align='center' justify='flex-end' gap={2}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Port:
            </Text>
            <Input
              value={mcpServerPort}
              onChange={handleMcpPortChange}
              onBlur={saveMcpPort}
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
