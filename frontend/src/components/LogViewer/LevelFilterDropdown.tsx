import { memo } from 'react'
import { Filter } from 'lucide-react'

import { Flex, Text } from '@chakra-ui/react'

import { ALL_LEVELS, LEVEL_COLORS } from './constants'
import { LogFilterAction, LogFilterDropdown } from './LogFilterDropdown'
import type {
  LevelFilterDropdownProps,
  LogFilterAccent,
  LogLevel,
} from './types'

const LEVEL_ACCENT: LogFilterAccent = {
  activeBg: 'blue.500/10',
  activeColor: 'blue.500',
  activeBorder: 'app.accentMuted',
  hoverBg: 'app.accentSubtle',
  hoverBorder: 'blue.500/40',
  badgeBg: 'blue.500',
  badgeColor: 'white',
  checkedBg: 'blue.500',
}

const renderLevel = (level: LogLevel) => (
  <Text
    fontSize='10px'
    fontWeight='medium'
    fontFamily='mono'
    color={LEVEL_COLORS[level].text}
  >
    {level}
  </Text>
)

function LevelFilterDropdownComponent({
  selectedLevels,
  onLevelChange,
}: LevelFilterDropdownProps) {
  return (
    <LogFilterDropdown
      ariaLabel='Filter by level'
      title='Levels'
      icon={<Filter size={12} />}
      accent={LEVEL_ACCENT}
      items={ALL_LEVELS}
      selected={selectedLevels}
      onChange={onLevelChange}
      renderItem={renderLevel}
      headerActions={
        <Flex gap={1}>
          <LogFilterAction
            label='All'
            onClick={() => onLevelChange([...ALL_LEVELS])}
          />
          <Text fontSize='9px' color='whiteAlpha.300'>
            |
          </Text>
          <LogFilterAction label='None' onClick={() => onLevelChange([])} />
        </Flex>
      }
      minW='120px'
    />
  )
}

export const LevelFilterDropdown = memo(LevelFilterDropdownComponent)
