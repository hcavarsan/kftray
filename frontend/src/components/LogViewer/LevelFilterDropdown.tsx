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
  activeBg: 'accent.subtle',
  activeColor: 'accent.fg',
  activeBorder: 'accent.muted',
  hoverBg: 'accent.subtle',
  hoverBorder: 'accent.emphasized',
  badgeBg: 'accent.solid',
  badgeColor: 'fg',
  checkedBg: 'accent.solid',
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
          <Text fontSize='9px' color='fg.faint'>
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
