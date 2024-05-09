import React from 'react'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Button,
} from '@chakra-ui/react'

import { BulkDeleteAlertDialogProps } from '../../types'

const BulkDeleteAlertDialog: React.FC<BulkDeleteAlertDialogProps> = ({
  isOpen,
  onClose,
  onConfirm,
}) => {
  const cancelRef = React.useRef<HTMLButtonElement>(null)

  return (
    <AlertDialog
      isOpen={isOpen}
      leastDestructiveRef={cancelRef}
      onClose={onClose}
    >
      <AlertDialogOverlay
        style={{ alignItems: 'flex-start', justifyContent: 'flex-start' }}
        bg='transparent'
      >
        <AlertDialogContent>
          <AlertDialogHeader fontSize='sm' fontWeight='bold'>
            Delete Config(s)
          </AlertDialogHeader>

          <AlertDialogBody>
            Are you sure you want to delete the selected config(s)? This action
            cannot be undone.
          </AlertDialogBody>

          <AlertDialogFooter>
            <Button ref={cancelRef} onClick={onClose}>
              Cancel
            </Button>
            <Button colorScheme='red' onClick={onConfirm} ml={3}>
              Delete
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialogOverlay>
    </AlertDialog>
  )
}

export default BulkDeleteAlertDialog
