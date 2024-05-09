import React, { useState } from 'react'
import { MdDelete } from 'react-icons/md'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Box,
  Button,
  IconButton,
  Tooltip,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { BulkDeleteButtonProps, Status } from '../../../types'
import useCustomToast from '../../CustomToast'

const BulkDeleteButton: React.FC<BulkDeleteButtonProps> = ({
  selectedConfigs,
  setSelectedConfigs,
  configs,
  setConfigs,
}) => {
  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const [isBulkAlertOpen, setIsBulkAlertOpen] = useState(false)
  const [configsToDelete, setConfigsToDelete] = useState<number[]>([])
  const toast = useCustomToast()

  const handleDeleteConfigs = (selectedIds: number[]) => {
    setConfigsToDelete(selectedIds)
    setIsBulkAlertOpen(true)
  }

  const confirmDeleteConfigs = async () => {
    if (!Array.isArray(configsToDelete) || !configsToDelete.length) {
      toast({
        title: 'Error',
        description: 'No configurations selected for deletion.',
        status: 'error',
      })

      return
    }

    try {
      await invoke('delete_configs', { ids: configsToDelete })

      const configsAfterDeletion = await invoke<Status[]>('get_configs')
      const runningStateMap = new Map(
        configs.map(conf => [conf.id, conf.isRunning]),
      )

      const updatedConfigs = configsAfterDeletion.map(conf => ({
        ...conf,
        isRunning: runningStateMap.get(conf.id) ?? false,
      }))

      setConfigs(updatedConfigs)
      setSelectedConfigs([])

      toast({
        title: 'Success',
        description: 'Configurations deleted successfully.',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configurations:', error)
      toast({
        title: 'Error',
        description: 'Failed to delete configurations: "unknown error"',
        status: 'error',
      })
    }

    setIsBulkAlertOpen(false)
  }

  return (
    <Box>
      {selectedConfigs.length > 0 && (
        <Tooltip
          label='Delete Configs'
          placement='top'
          fontSize='xs'
          lineHeight='tight'
        >
          <IconButton
            colorScheme='red'
            variant='outline'
            onClick={() =>
              handleDeleteConfigs(selectedConfigs.map(config => config.id))
            }
            size='sm'
            aria-label='Delete selected configs'
            borderColor='gray.700'
            icon={<MdDelete />}
            ml={2}
          />
        </Tooltip>
      )}

      <AlertDialog
        isOpen={isBulkAlertOpen}
        leastDestructiveRef={cancelRef}
        onClose={() => setIsBulkAlertOpen(false)}
      >
        <AlertDialogOverlay
          style={{ alignItems: 'flex-start', justifyContent: 'flex-start' }}
          bg='transparent'
        >
          <AlertDialogContent>
            <AlertDialogHeader fontSize='xs' fontWeight='bold'>
              Delete Config(s)
            </AlertDialogHeader>

            <AlertDialogBody fontSize='xs'>
              Are you sure you want to delete the selected config(s)? This
              action cannot be undone.
            </AlertDialogBody>

            <AlertDialogFooter>
              <Button
                ref={cancelRef}
                onClick={() => setIsBulkAlertOpen(false)}
                size='xs'
              >
                Cancel
              </Button>
              <Button
                colorScheme='red'
                onClick={confirmDeleteConfigs}
                ml={3}
                size='xs'
              >
                Delete
              </Button>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialogOverlay>
      </AlertDialog>
    </Box>
  )
}

export default BulkDeleteButton
