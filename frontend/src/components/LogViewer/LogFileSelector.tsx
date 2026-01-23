import { memo, useCallback, useMemo } from 'react'
import { ChevronDown, Trash2 } from 'lucide-react'

import { Box, Flex, Menu, Portal, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'

import { COLORS } from './constants'
import type { LogFileInfo } from './types'

interface LogFileSelectorProps {
  logFiles: LogFileInfo[]
  selectedFile: string | null
  onFileSelect: (filename: string | null) => void
  onDeleteFile?: (filename: string) => void
  isLoading?: boolean
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`
  }

  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatAge(days: number): string {
  if (days === 0) {
    return 'Today'
  }
  if (days === 1) {
    return '1 day ago'
  }

  return `${days} days ago`
}

function LogFileSelectorComponent({
  logFiles,
  selectedFile,
  onFileSelect,
  onDeleteFile,
  isLoading,
}: LogFileSelectorProps) {
  const selectedFileInfo = useMemo(() => {
    if (!selectedFile) {
      return logFiles.find(f => f.is_current) || logFiles[0]
    }

    return logFiles.find(f => f.filename === selectedFile) || logFiles[0]
  }, [logFiles, selectedFile])

  const handleSelect = useCallback(
    (filename: string) => {
      const file = logFiles.find(f => f.filename === filename)

      if (file?.is_current) {
        onFileSelect(null)
      } else {
        onFileSelect(filename)
      }
    },
    [logFiles, onFileSelect],
  )

  const handleDelete = useCallback(
    (e: React.MouseEvent, filename: string) => {
      e.stopPropagation()
      onDeleteFile?.(filename)
    },
    [onDeleteFile],
  )

  if (logFiles.length === 0) {
    return (
      <Text fontSize='xs' color='whiteAlpha.500'>
        No log files
      </Text>
    )
  }

  return (
    <Menu.Root>
      <Menu.Trigger asChild>
        <Button
          size='xs'
          variant='outline'
          height='28px'
          px={2}
          bg='transparent'
          color='whiteAlpha.800'
          borderColor={COLORS.borderDefault}
          _hover={{
            bg: 'whiteAlpha.50',
            borderColor: COLORS.borderHover,
          }}
          disabled={isLoading}
        >
          <Flex align='center' gap={1.5}>
            <Box
              w='6px'
              h='6px'
              borderRadius='full'
              bg={
                selectedFileInfo?.is_current
                  ? COLORS.accentBlue
                  : 'whiteAlpha.400'
              }
              flexShrink={0}
            />
            <Text
              fontSize='11px'
              maxW='160px'
              overflow='hidden'
              textOverflow='ellipsis'
              whiteSpace='nowrap'
            >
              {selectedFileInfo?.is_current
                ? 'Current Session'
                : selectedFileInfo?.created_at || 'Select'}
            </Text>
            <Text fontSize='10px' color='whiteAlpha.500'>
              ({selectedFileInfo ? formatFileSize(selectedFileInfo.size) : ''})
            </Text>
            <ChevronDown size={12} />
          </Flex>
        </Button>
      </Menu.Trigger>
      <Portal>
        <Menu.Positioner>
          <Menu.Content
            bg={COLORS.bgSecondary}
            border='1px solid'
            borderColor={COLORS.borderDefault}
            minW='240px'
            maxH='250px'
            overflowY='auto'
            py={0.5}
          >
            <Box
              px={2}
              py={1}
              borderBottom='1px solid'
              borderBottomColor={COLORS.borderSubtle}
            >
              <Text fontSize='10px' color='whiteAlpha.500' fontWeight='medium'>
                Log Files ({logFiles.length})
              </Text>
            </Box>
            {logFiles.map(file => {
              const isSelected = selectedFileInfo?.filename === file.filename

              return (
                <Menu.Item
                  key={file.filename}
                  value={file.filename}
                  onClick={() => handleSelect(file.filename)}
                  bg={isSelected ? 'rgba(59, 130, 246, 0.1)' : 'transparent'}
                  _hover={{
                    bg: isSelected
                      ? 'rgba(59, 130, 246, 0.15)'
                      : 'whiteAlpha.50',
                  }}
                  py={1.5}
                  px={2}
                >
                  <Flex align='center' justify='space-between' w='100%'>
                    <Flex align='center' gap={2} flex={1} overflow='hidden'>
                      <Box
                        w='6px'
                        h='6px'
                        borderRadius='full'
                        bg={
                          file.is_current ? COLORS.accentBlue : 'whiteAlpha.300'
                        }
                        flexShrink={0}
                      />
                      <Text
                        fontSize='11px'
                        color={
                          file.is_current
                            ? COLORS.accentBlue
                            : isSelected
                              ? COLORS.accentBlue
                              : 'whiteAlpha.900'
                        }
                        fontWeight={file.is_current ? 'medium' : 'normal'}
                        overflow='hidden'
                        textOverflow='ellipsis'
                        whiteSpace='nowrap'
                      >
                        {file.is_current ? 'Current Session' : file.created_at}
                      </Text>
                      <Text fontSize='10px' color='whiteAlpha.400'>
                        {formatFileSize(file.size)}
                      </Text>
                      {!file.is_current && (
                        <Text fontSize='10px' color='whiteAlpha.400'>
                          • {formatAge(file.age_days)}
                        </Text>
                      )}
                    </Flex>
                    {!file.is_current && onDeleteFile && (
                      <Tooltip content='Delete' portalled>
                        <Box
                          as='button'
                          p={0.5}
                          borderRadius='3px'
                          color='whiteAlpha.400'
                          _hover={{
                            bg: 'rgba(229, 62, 62, 0.1)',
                            color: 'red.400',
                          }}
                          _focus={{ outline: 'none', boxShadow: 'none' }}
                          _focusVisible={{ outline: 'none', boxShadow: 'none' }}
                          onClick={e => handleDelete(e, file.filename)}
                        >
                          <Trash2 size={11} />
                        </Box>
                      </Tooltip>
                    )}
                  </Flex>
                </Menu.Item>
              )
            })}
          </Menu.Content>
        </Menu.Positioner>
      </Portal>
    </Menu.Root>
  )
}

export const LogFileSelector = memo(LogFileSelectorComponent)
