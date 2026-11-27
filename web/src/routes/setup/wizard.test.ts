// Browsers (Chromium's ICU) list some zones under legacy names; Leavo shows current IANA names.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { canonicalZone, timeZones } from './wizard'

afterEach(() => vi.restoreAllMocks())

describe('time zones', () => {
  it('maps legacy names to current IANA names', () => {
    expect(canonicalZone('Asia/Calcutta')).toBe('Asia/Kolkata')
    expect(canonicalZone('Asia/Katmandu')).toBe('Asia/Kathmandu')
    expect(canonicalZone('Europe/Kiev')).toBe('Europe/Kyiv')
    expect(canonicalZone('Europe/London')).toBe('Europe/London')
  })

  it('lists current names, sorted and without duplicates', () => {
    vi.spyOn(Intl, 'supportedValuesOf').mockReturnValue([
      'Asia/Calcutta',
      'Asia/Kolkata',
      'Africa/Asmera',
      'UTC',
    ])
    expect(timeZones()).toEqual(['Africa/Asmara', 'Asia/Kolkata', 'UTC'])
  })
})
