import { Box, Dialog } from '@chakra-ui/react'

import {
  AppDialog,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'

export type HelperAction = 'install' | 'uninstall'

export interface HelperActionResult {
  success: boolean
  message: string
  action: HelperAction
}

interface HelperResultDialogProps {
  result: HelperActionResult
  onClose: () => void
}

export function HelperResultDialog({
  result,
  onClose,
}: HelperResultDialogProps) {
  return (
    <AppDialog
      title={
        result.success
          ? result.action === 'install'
            ? 'Installation Successful'
            : 'Uninstallation Successful'
          : result.action === 'install'
            ? 'Installation Failed'
            : 'Uninstallation Failed'
      }
      onClose={onClose}
      maxWidth='400px'
    >
      <Dialog.Body p={3}>
        <Box
          p={3}
          bg={result.success ? 'success.subtle' : 'danger.subtle'}
          borderRadius='md'
          border='1px solid'
          borderColor={result.success ? 'success.border' : 'danger.muted'}
        >
          <Box
            fontSize='xs'
            color={result.success ? 'success.fg' : 'danger.fg'}
          >
            {result.message}
          </Box>
        </Box>
      </Dialog.Body>
      <AppDialogFooter>
        <DialogCancelButton label='Close' onClick={onClose} />
      </AppDialogFooter>
    </AppDialog>
  )
}
