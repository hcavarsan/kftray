import type { ChangeEvent } from 'react'

import { Input, Stack, Text } from '@chakra-ui/react'

import { Radio, RadioGroup } from '@/components/ui/radio'
import type { AuthMethod } from '@/types'

interface AuthFieldsProps {
  authMethod: AuthMethod
  gitToken: string
  onAuthMethodChange: (details: { value: string | null }) => void
  onGitTokenChange: (value: string) => void
}

export function AuthFields({
  authMethod,
  gitToken,
  onAuthMethodChange,
  onGitTokenChange,
}: AuthFieldsProps) {
  return (
    <Stack gap={2}>
      <Text fontSize='xs' color='gray.400'>
        Authentication Method
      </Text>
      <Stack
        direction='row'
        gap={2}
        bg='app.panel'
        p={2}
        borderRadius='md'
        border='1px solid'
        borderColor='app.border'
      >
        <RadioGroup
          value={authMethod}
          onValueChange={onAuthMethodChange}
          size='xs'
        >
          <Stack direction='row' gap={2}>
            <Radio value='none'>
              <Text fontSize='xs' color='gray.400'>
                Public Repository
              </Text>
            </Radio>
            <Radio value='system'>
              <Text fontSize='xs' color='gray.400'>
                Use System Git Credentials
              </Text>
            </Radio>
            <Radio value='token'>
              <Text fontSize='xs' color='gray.400'>
                GitHub Token
              </Text>
            </Radio>
          </Stack>
        </RadioGroup>
      </Stack>

      {authMethod === 'token' && (
        <Input
          type='password'
          value={gitToken}
          onChange={(e: ChangeEvent<HTMLInputElement>) =>
            onGitTokenChange(e.target.value)
          }
          placeholder='Enter your GitHub token'
          bg='app.panel'
          borderColor='app.border'
          _hover={{
            borderColor: 'app.divider',
            bg: 'app.panel',
          }}
          height='30px'
          fontSize='12px'
        />
      )}
    </Stack>
  )
}
