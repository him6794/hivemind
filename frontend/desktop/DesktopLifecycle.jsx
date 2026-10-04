import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import {
  AlertDialog, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
  AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { createCloseController, getDesktopBridge, motionDelay } from './desktopLifecycle.mjs';
import './desktop-lifecycle.css';

export function DesktopLifecycle({ children, role }) {
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState('');
  const controller = useRef(null);
  const previousFocus = useRef(null);
  const alive = useRef(false);
  const dialogOpen = useRef(false);

  useEffect(() => {
    alive.current = true;
    const root = document.documentElement;
    controller.current = createCloseController({
      bridge: getDesktopBridge,
      delay: motionDelay(),
      setVisibility: (state) => { root.dataset.windowState = state; },
    });
    const show = () => {
      root.dataset.windowState = 'visible';
      root.classList.remove('desktop-enter');
      void root.offsetWidth;
      root.classList.add('desktop-enter');
    };
    const request = () => {
      if (!getDesktopBridge() || controller.current.busy || dialogOpen.current) return;
      dialogOpen.current = true;
      previousFocus.current = document.activeElement;
      setError('');
      setOpen(true);
    };
    window.addEventListener('hivemind:close-requested', request);
    window.addEventListener('hivemind:window-shown', show);
    show();
    const bridge = getDesktopBridge();
    if (bridge) Promise.resolve().then(() => bridge.ready()).catch(() => {});
    return () => {
      alive.current = false;
      window.removeEventListener('hivemind:close-requested', request);
      window.removeEventListener('hivemind:window-shown', show);
      delete root.dataset.windowState;
      root.classList.remove('desktop-enter');
    };
  }, []);

  async function resolve(action) {
    if (!controller.current || controller.current.busy) return;
    setPending(true);
    setError('');
    try {
      const completed = await controller.current.resolve(action);
      if (completed && alive.current) {
        dialogOpen.current = false;
        setOpen(false);
      }
    } catch {
      if (alive.current) setError('Could not close the window. Try again.');
    } finally {
      if (alive.current) setPending(false);
    }
  }

  return (
    <>
      {children}
      <AlertDialog open={open} onOpenChange={(next) => { if (!next && !pending) void resolve('cancel'); }}>
        <AlertDialogContent
          className="w-[calc(100%-2rem)] max-w-md max-h-[calc(100dvh-2rem)] overflow-y-auto rounded-lg"
          onEscapeKeyDown={(event) => { if (pending) event.preventDefault(); }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            previousFocus.current?.focus?.();
          }}
        >
          <AlertDialogHeader>
            <AlertDialogTitle>Keep {role} running?</AlertDialogTitle>
            <AlertDialogDescription>Keep the service in the background, or quit this client.</AlertDialogDescription>
          </AlertDialogHeader>
          {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
          <AlertDialogFooter className="gap-2 sm:space-x-0">
            <AlertDialogCancel asChild><Button variant="outline" className="min-h-11" disabled={pending}>Cancel</Button></AlertDialogCancel>
            <Button variant="destructive" className="min-h-11" disabled={pending} onClick={() => void resolve('quit')}>Quit</Button>
            <Button className="min-h-11" disabled={pending} onClick={() => void resolve('background')}>Keep in background</Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

export default DesktopLifecycle;
