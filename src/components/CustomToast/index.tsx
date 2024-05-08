import { useToast } from '@chakra-ui/react'

import { ShowToastParams } from '../../types'

import CustomToast from './CustomToast'

const useCustomToast = () => {
  const toast = useToast()

  const showToast = ({
    title,
    description,
    status = 'error',
    duration = 3000,
    isClosable = true,
    position = 'top-right',
  }: ShowToastParams) => {
    toast({
      duration,
      isClosable,
      position,
      render: () => (
        <CustomToast title={title} description={description} status={status} />
      ),
    })
  }

  return showToast
}

export default useCustomToast
