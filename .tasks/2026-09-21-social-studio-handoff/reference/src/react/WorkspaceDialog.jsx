import React, { useEffect, useId, useRef } from 'react';
import { createPortal } from 'react-dom';
import { Icon } from './Icon.jsx';
/** Mount only while open. Native dialog supplies top-layer rendering and inert background. */
export function WorkspaceDialog({ title, description, onClose, busy = false, small = false, children }) {
    const ref = useRef(null);
    const titleId = useId();
    const descriptionId = useId();
    useEffect(() => {
        const dialog = ref.current;
        const previousFocus = document.activeElement;
        const previousOverflow = document.body.style.overflow;
        document.body.style.overflow = 'hidden';
        dialog.showModal();
        return () => {
            dialog.close();
            document.body.style.overflow = previousOverflow;
            if (previousFocus instanceof HTMLElement && previousFocus.isConnected)
                previousFocus.focus({ preventScroll: true });
        };
    }, []);
    function containFocus(event) {
        if (event.key !== 'Tab')
            return;
        const items = [...ref.current.querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex="0"]')].filter(element => element.getClientRects().length);
        const first = items[0], last = items.at(-1);
        if ((event.shiftKey && document.activeElement === first) || (!event.shiftKey && document.activeElement === last)) {
            event.preventDefault();
            (event.shiftKey ? last : first)?.focus();
        }
    }
    return createPortal(<dialog ref={ref} onKeyDown={containFocus} className={`modal ${small ? 'small' : ''}`} aria-labelledby={titleId} aria-describedby={description ? descriptionId : undefined} aria-busy={busy} onCancel={event => { event.preventDefault(); if (!busy)
        onClose(); }}>
      <header className="modal-header">
        <div className="min-w-0">{description && <p id={descriptionId} className="mb-1 text-sm text-accent">{description}</p>}<h2 id={titleId} className="truncate text-lg font-medium">{title}</h2></div>
        <button type="button" onClick={onClose} disabled={busy} aria-label="Dialog schließen" className="btn btn-ghost icon-btn"><Icon name="x"/></button>
      </header>
      <div className="modal-body">{children}</div>
    </dialog>, document.body);
}
