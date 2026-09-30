import { FilterMenu } from '@/components/HeaderMenu/FilterMenu'
import { GroupByMenu } from '@/components/HeaderMenu/GroupByMenu'
import type { ConfigView, Facet } from '@/types'

interface ViewControlsProps {
  view: ConfigView | null
  facets: Facet[]
  setView: (view: ConfigView) => void
}

export const ViewControls = ({ view, facets, setView }: ViewControlsProps) => {
  if (!view) {
    return null
  }

  return (
    <>
      <GroupByMenu view={view} facets={facets} setView={setView} />
      <FilterMenu view={view} facets={facets} setView={setView} />
    </>
  )
}
