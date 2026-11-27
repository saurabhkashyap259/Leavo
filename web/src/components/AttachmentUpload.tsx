// Supporting document (FR-030): uploaded before submitting; the server checks type and size.
import { useMutation } from '@tanstack/react-query'
import { useId } from 'react'
import { useTranslation } from 'react-i18next'
import { errorMessage } from '../api/client'
import { uploadAttachment, type Uploaded } from '../api/leaves'

export function AttachmentUpload({
  value,
  onChange,
  required,
}: {
  value: Uploaded | null
  onChange: (v: Uploaded | null) => void
  /** Message shown when the request needs a document (ATTACHMENT_REQUIRED). */
  required?: string | null
}) {
  const { t } = useTranslation()
  const id = useId()
  const upload = useMutation({ mutationFn: uploadAttachment, onSuccess: onChange })

  return (
    <div data-field="attachment" className="space-y-1">
      <label htmlFor={id} className="block text-sm font-medium">
        {t('leave.attachment.label')}
      </label>
      {value ? (
        <p className="flex items-center gap-2 text-sm">
          <span>{value.name}</span>
          <button
            type="button"
            className="rounded border border-line px-2 py-0.5"
            onClick={() => onChange(null)}
          >
            {t('leave.attachment.remove')}
          </button>
        </p>
      ) : (
        <input
          id={id}
          type="file"
          accept="application/pdf,image/png,image/jpeg,image/webp,image/heic,image/heif,.heic"
          aria-describedby={`${id}-hint`}
          disabled={upload.isPending}
          onChange={(e) => {
            const file = e.target.files?.[0]
            if (file) upload.mutate(file)
            e.target.value = ''
          }}
          className="block text-sm"
        />
      )}
      {upload.isPending && <p className="text-sm">{t('leave.attachment.uploading')}</p>}
      {upload.error && <p className="text-sm text-red-700">{errorMessage(upload.error, t)}</p>}
      {required && !value && <p className="text-sm font-medium text-amber-800">{required}</p>}
      <p id={`${id}-hint`} className="text-xs text-gray-600">
        {t('leave.attachment.hint')}
      </p>
    </div>
  )
}
