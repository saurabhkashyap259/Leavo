import { useId, type ReactNode } from 'react'

/** A labelled input with an optional error and help text (accessible by label). */
export function Field({
  label,
  error,
  help,
  children,
}: {
  label: string
  error?: string
  help?: string
  children: (id: string, describedBy?: string) => ReactNode
}) {
  const id = useId()
  const hint = `${id}-hint`
  return (
    <div>
      <label htmlFor={id} className="block text-sm font-medium">
        {label}
      </label>
      {children(id, error || help ? hint : undefined)}
      {error ? (
        <p id={hint} className="mt-1 text-sm text-red-700">
          {error}
        </p>
      ) : help ? (
        <p id={hint} className="mt-1 text-sm text-gray-600">
          {help}
        </p>
      ) : null}
    </div>
  )
}

export function PrimaryButton({
  children,
  busy,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { busy?: boolean }) {
  return (
    <button
      {...rest}
      disabled={busy || rest.disabled}
      className="rounded bg-accent px-4 py-2 font-medium text-white disabled:opacity-60"
    >
      {children}
    </button>
  )
}
