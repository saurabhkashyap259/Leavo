import type { ReactNode } from 'react'

export function AuthCard({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex min-h-screen items-start justify-center bg-surface px-4 pt-16 sm:pt-24">
      <div className="w-full max-w-sm rounded-lg bg-white p-6 shadow-sm">
        <h1 className="mb-6 text-xl font-semibold">{title}</h1>
        {children}
      </div>
    </div>
  )
}
