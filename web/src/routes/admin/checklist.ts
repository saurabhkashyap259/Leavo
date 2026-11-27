// Which checklist pages the admin has opened, kept in this browser.
import type { ChecklistItem } from '../../components/SetupChecklist'

const SEEN = 'leavo.checklist.seen'

/** Remember that the admin opened a checklist page (holidays, approvers and invites join in later stories). */
export function markChecklistSeen(key: ChecklistItem['key']) {
  try {
    const seen = new Set<string>(JSON.parse(localStorage.getItem(SEEN) ?? '[]'))
    seen.add(key)
    localStorage.setItem(SEEN, JSON.stringify([...seen]))
  } catch {
    /* storage unavailable: the checklist just stays visible */
  }
}

export function seen(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(SEEN) ?? '[]'))
  } catch {
    return new Set()
  }
}
