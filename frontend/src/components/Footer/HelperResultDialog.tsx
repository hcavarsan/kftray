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
          bg={result.success ? 'status.success.bg' : 'status.danger.bg'}
          borderRadius='md'
          border='1px solid'
          borderColor={
            result.success ? 'status.success.border' : 'status.danger.border'
          }
        >
          <Box fontSize='xs' color={result.success ? 'green.300' : 'red.300'}>
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
