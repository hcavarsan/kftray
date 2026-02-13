import React from 'react'
import { FileCode, FolderOpen } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'
import { save } from '@tauri-apps/plugin-dialog'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'

interface EnvAutoSyncSettingsProps {
  envAutoSyncEnabled: boolean
  setEnvAutoSyncEnabled: (enabled: boolean) => void
  envAutoSyncPath: string
  setEnvAutoSyncPath: (path: string) => void
  isLoading: boolean
}

const EnvAutoSyncSettings: React.FC<EnvAutoSyncSettingsProps> = ({
  envAutoSyncEnabled,
  setEnvAutoSyncEnabled,
  envAutoSyncPath,
  setEnvAutoSyncPath,
  isLoading,
}) => {
  const selectEnvPath = async () => {
    try {
      await invoke('open_save_dialog')
      const filePath = await save({
        defaultPath: envAutoSyncPath || '.env',
        filters: [
          { name: 'Environment Files', extensions: ['env'] },
          { name: 'All Files', extensions: ['*'] },
        ],
      })

      if (filePath) {
        setEnvAutoSyncPath(filePath)
      }
    } catch (error) {
      console.error('Error selecting env file path:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to select file path',
        duration: 3000,
      })
    } finally {
      await invoke('close_save_dialog')
    }
  }

  return (
    <>
      {/* Left Column - .env Auto-Sync Enable */}
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
            as={FileCode}
            width='10px'
            height='10px'
            color='green.400'
          />
          <Text fontSize='sm' fontWeight='500' color='white'>
            .env Auto-Sync
          </Text>
          <Box
            width='5px'
            height='5px'
            borderRadius='full'
            bg={envAutoSyncEnabled && envAutoSyncPath ? 'green.400' : 'gray.500'}
            title={envAutoSyncEnabled && envAutoSyncPath ? 'Active' : 'Inactive'}
          />
        </Flex>
        <Text
          fontSize='xs'
          color='whiteAlpha.600'
          lineHeight='1.3'
          flex='1'
        >
          Auto-update .env file when port-forwards start or stop.
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
              checked={envAutoSyncEnabled}
              onCheckedChange={e =>
                setEnvAutoSyncEnabled(e.checked === true)
              }
              disabled={isLoading}
              size='sm'
            />
          </Flex>
        </Box>
      </Box>

      {/* Right Column - .env File Path */}
      <Box
        bg='#161616'
        p={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        display='flex'
        flexDirection='column'
        height='100%'
        opacity={envAutoSyncEnabled ? 1 : 0.5}
      >
        <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
          .env File Path
        </Text>
        <Text
          fontSize='xs'
          color='whiteAlpha.600'
          lineHeight='1.3'
          flex='1'
          wordBreak='break-all'
        >
          {envAutoSyncPath || 'No path selected'}
        </Text>
        <Box
          borderTop='1px solid rgba(255, 255, 255, 0.06)'
          mt={3}
          pt={3}
        >
          <Flex align='center' justify='flex-end'>
            <Button
              size='2xs'
              variant='outline'
              onClick={selectEnvPath}
              disabled={isLoading || !envAutoSyncEnabled}
              height='18px'
              fontSize='10px'
              color='whiteAlpha.600'
              borderColor='rgba(255, 255, 255, 0.1)'
              _hover={{
                borderColor: 'rgba(255, 255, 255, 0.2)',
                bg: 'whiteAlpha.50',
              }}
              px={1.5}
            >
              <Box as={FolderOpen} width='8px' height='8px' mr={0.5} />
              Browse...
            </Button>
          </Flex>
        </Box>
      </Box>
    </>
  )
}

export default EnvAutoSyncSettings
