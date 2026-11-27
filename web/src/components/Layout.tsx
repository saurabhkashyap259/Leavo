import { Link, Outlet } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import type { Me, Role } from '../api/me'
import { navItemsFor } from './nav'

export function Layout({ me }: { me: Me }) {
  const { t } = useTranslation()
  return (
    <div className="min-h-screen bg-surface text-ink md:flex">
      <nav
        aria-label={t('nav.main')}
        className="border-b border-line bg-white md:min-h-screen md:w-56 md:border-r md:border-b-0"
      >
        <div className="px-4 py-4 text-lg font-semibold">{t('app.name')}</div>
        <ul className="flex gap-1 overflow-x-auto px-2 pb-2 md:flex-col md:pb-0">
          {navItemsFor(me.role as Role).map((item) => (
            <li key={item.to}>
              <Link
                to={item.to}
                className="block rounded px-3 py-2 text-sm whitespace-nowrap hover:bg-surface focus-visible:outline-2 focus-visible:outline-accent"
                activeProps={{ className: 'bg-surface font-medium', 'aria-current': 'page' }}
              >
                {t(item.label)}
              </Link>
            </li>
          ))}
        </ul>
        <div className="px-2 pb-3 md:mt-4 md:border-t md:border-line md:pt-3">
          <Link
            to="/profile"
            className="block rounded px-3 py-2 text-sm hover:bg-surface"
            activeProps={{ className: 'bg-surface font-medium', 'aria-current': 'page' }}
          >
            {me.full_name} · {t('nav.profile')}
          </Link>
        </div>
      </nav>
      <main className="flex-1 p-4 md:p-8">
        <Outlet />
      </main>
    </div>
  )
}
