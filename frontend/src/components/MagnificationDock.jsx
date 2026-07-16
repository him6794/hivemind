import React, { Children, cloneElement, useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion, useMotionValue, useSpring, useTransform } from 'framer-motion';
import { colors, font } from '../theme';

function DockItem({
  children,
  className = '',
  onClick,
  mouseX,
  spring,
  distance,
  magnification,
  baseItemSize,
}) {
  const ref = useRef(null);
  const isHovered = useMotionValue(0);

  const mouseDistance = useTransform(mouseX, (val) => {
    const rect = ref.current?.getBoundingClientRect() ?? { x: 0, width: baseItemSize };
    return val - rect.x - baseItemSize / 2;
  });

  const targetSize = useTransform(mouseDistance, [-distance, 0, distance], [baseItemSize, magnification, baseItemSize]);
  const size = useSpring(targetSize, spring);

  return (
    <motion.div
      ref={ref}
      style={{ width: size, height: size, position: 'relative' }}
      onHoverStart={() => isHovered.set(1)}
      onHoverEnd={() => isHovered.set(0)}
      onFocus={() => isHovered.set(1)}
      onBlur={() => isHovered.set(0)}
      onClick={onClick}
      className={className}
      tabIndex={0}
      role="button"
      aria-haspopup="true"
    >
      <div
        style={{
          width: '100%',
          height: '100%',
          borderRadius: 999,
          display: 'grid',
          placeItems: 'center',
          background: 'linear-gradient(160deg, rgba(255,255,255,0.14), rgba(6,182,212,0.22))',
          border: '1px solid rgba(255,255,255,0.16)',
          boxShadow: '0 10px 28px rgba(15,23,42,0.28)',
          cursor: 'pointer',
          color: colors.white,
        }}
      >
        {Children.map(children, (child) =>
          React.isValidElement(child) ? cloneElement(child, { isHovered }) : child,
        )}
      </div>
    </motion.div>
  );
}

function DockLabel({ children, isHovered }) {
  const [isVisible, setIsVisible] = useState(false);

  useEffect(() => {
    if (!isHovered) return undefined;
    const unsubscribe = isHovered.on('change', (latest) => setIsVisible(latest === 1));
    return () => unsubscribe();
  }, [isHovered]);

  return (
    <AnimatePresence>
      {isVisible ? (
        <motion.div
          initial={{ opacity: 0, y: 0 }}
          animate={{ opacity: 1, y: -10 }}
          exit={{ opacity: 0, y: 0 }}
          transition={{ duration: 0.2 }}
          role="tooltip"
          style={{
            position: 'absolute',
            top: -34,
            left: '50%',
            transform: 'translateX(-50%)',
            whiteSpace: 'nowrap',
            borderRadius: 10,
            border: '1px solid rgba(255,255,255,0.12)',
            background: 'rgba(15,23,42,0.92)',
            color: colors.white,
            padding: '6px 10px',
            fontSize: 12,
            fontFamily: font.sans,
            boxShadow: '0 10px 24px rgba(0,0,0,0.25)',
            pointerEvents: 'none',
          }}
        >
          {children}
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

function DockIcon({ children }) {
  return <div style={{ display: 'grid', placeItems: 'center', color: colors.white }}>{children}</div>;
}

export default function MagnificationDock({
  items,
  className = '',
  spring = { mass: 0.1, stiffness: 150, damping: 12 },
  magnification = 74,
  distance = 160,
  panelHeight = 68,
  dockHeight = 180,
  baseItemSize = 50,
}) {
  const mouseX = useMotionValue(Infinity);
  const isHovered = useMotionValue(0);
  const maxHeight = useMemo(() => Math.max(dockHeight, magnification + magnification / 2 + 4), [dockHeight, magnification]);
  const heightRow = useTransform(isHovered, [0, 1], [panelHeight, maxHeight]);
  const height = useSpring(heightRow, spring);

  return (
    <motion.div style={{ height, width: '100%' }} className={className}>
      <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'flex-end', height: '100%' }}>
        <motion.div
          onMouseMove={({ pageX }) => {
            isHovered.set(1);
            mouseX.set(pageX);
          }}
          onMouseLeave={() => {
            isHovered.set(0);
            mouseX.set(Infinity);
          }}
          role="toolbar"
          aria-label="Application dock"
          style={{
            display: 'flex',
            alignItems: 'flex-end',
            gap: 12,
            width: 'fit-content',
            maxWidth: '100%',
            padding: '10px 16px 12px',
            borderRadius: 28,
            border: '1px solid rgba(255,255,255,0.12)',
            background: 'rgba(30, 41, 59, 0.55)',
            backdropFilter: 'blur(16px)',
            WebkitBackdropFilter: 'blur(16px)',
            boxShadow: '0 18px 48px rgba(0,0,0,0.28)',
            height: panelHeight,
          }}
        >
          {items.map((item, index) => (
            <DockItem
              key={index}
              onClick={item.onClick}
              className={item.className}
              mouseX={mouseX}
              spring={spring}
              distance={distance}
              magnification={magnification}
              baseItemSize={baseItemSize}
            >
              <DockIcon>{item.icon}</DockIcon>
              <DockLabel>{item.label}</DockLabel>
            </DockItem>
          ))}
        </motion.div>
      </div>
    </motion.div>
  );
}
