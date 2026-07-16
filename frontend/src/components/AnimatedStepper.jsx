import React, { Children, useLayoutEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { Check } from 'lucide-react';
import { colors, font, primaryButton } from '../theme';

export function AnimatedStepper({
  children,
  initialStep = 1,
  onStepChange = () => {},
  onFinalStepCompleted = () => {},
  backButtonText = 'Back',
  nextButtonText = 'Continue',
  completeButtonText = 'Finish',
  disableStepIndicators = false,
  className = '',
  tone = 'dark',
}) {
  const [currentStep, setCurrentStep] = useState(initialStep);
  const [direction, setDirection] = useState(0);
  const stepsArray = Children.toArray(children);
  const totalSteps = stepsArray.length;
  const isCompleted = currentStep > totalSteps;
  const isLastStep = currentStep === totalSteps;
  const light = tone === 'light';

  const updateStep = (newStep) => {
    setCurrentStep(newStep);
    if (newStep > totalSteps) onFinalStepCompleted();
    else onStepChange(newStep);
  };

  const handleBack = () => {
    if (currentStep > 1) {
      setDirection(-1);
      updateStep(currentStep - 1);
    }
  };

  const handleNext = () => {
    if (!isLastStep) {
      setDirection(1);
      updateStep(currentStep + 1);
    }
  };

  const handleComplete = () => {
    setDirection(1);
    updateStep(totalSteps + 1);
  };

  return (
    <div className={className} style={{ width: '100%' }}>
      <div
        style={{
          width: '100%',
          overflow: 'hidden',
          borderRadius: 28,
          border: light ? '1px solid rgba(15,23,42,0.08)' : '1px solid rgba(255,255,255,0.1)',
          background: light ? colors.white : 'rgba(30, 41, 59, 0.72)',
          backdropFilter: light ? 'none' : 'blur(16px)',
          WebkitBackdropFilter: light ? 'none' : 'blur(16px)',
          boxShadow: light ? '0 18px 40px rgba(15,23,42,0.04)' : '0 24px 60px rgba(0,0,0,0.22)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', padding: '28px 28px 12px' }}>
          {stepsArray.map((_, index) => {
            const stepNumber = index + 1;
            const isNotLastStep = index < totalSteps - 1;
            return (
              <React.Fragment key={stepNumber}>
                <StepIndicator
                  step={stepNumber}
                  currentStep={currentStep}
                  disableStepIndicators={disableStepIndicators}
                  tone={tone}
                  onClickStep={(clicked) => {
                    setDirection(clicked > currentStep ? 1 : -1);
                    updateStep(clicked);
                  }}
                />
                {isNotLastStep ? <StepConnector isComplete={currentStep > stepNumber} tone={tone} /> : null}
              </React.Fragment>
            );
          })}
        </div>

        <StepContentWrapper isCompleted={isCompleted} currentStep={currentStep} direction={direction}>
          {React.isValidElement(stepsArray[currentStep - 1])
            ? React.cloneElement(stepsArray[currentStep - 1], { tone })
            : stepsArray[currentStep - 1]}
        </StepContentWrapper>

        {!isCompleted ? (
          <div style={{ padding: '8px 28px 28px' }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: currentStep !== 1 ? 'space-between' : 'flex-end' }}>
              {currentStep !== 1 ? (
                <button
                  type="button"
                  onClick={handleBack}
                  style={{
                    border: 0,
                    background: 'transparent',
                    color: light ? colors.muted : 'rgba(226,232,240,0.7)',
                    cursor: 'pointer',
                    fontSize: 14,
                    fontWeight: 600,
                    fontFamily: font.sans,
                  }}
                >
                  {backButtonText}
                </button>
              ) : null}
              <button
                type="button"
                className="hm-btn"
                onClick={isLastStep ? handleComplete : handleNext}
                style={{ ...primaryButton, minWidth: 128 }}
              >
                {isLastStep ? completeButtonText : nextButtonText}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function StepContentWrapper({ isCompleted, currentStep, direction, children }) {
  const [parentHeight, setParentHeight] = useState(0);

  return (
    <motion.div
      style={{ position: 'relative', overflow: 'hidden', padding: '0 28px' }}
      animate={{ height: isCompleted ? 0 : parentHeight || 'auto' }}
      transition={{ type: 'spring', damping: 25, stiffness: 200 }}
    >
      <AnimatePresence initial={false} mode="wait" custom={direction}>
        {!isCompleted ? (
          <SlideTransition key={currentStep} direction={direction} onHeightReady={setParentHeight}>
            {children}
          </SlideTransition>
        ) : null}
      </AnimatePresence>
    </motion.div>
  );
}

function SlideTransition({ children, direction, onHeightReady }) {
  const containerRef = useRef(null);

  useLayoutEffect(() => {
    if (containerRef.current) onHeightReady(containerRef.current.offsetHeight);
  }, [children, onHeightReady]);

  return (
    <motion.div
      ref={containerRef}
      custom={direction}
      variants={{
        enter: (dir) => ({ x: dir >= 0 ? 24 : -24, opacity: 0 }),
        center: { x: 0, opacity: 1 },
        exit: (dir) => ({ x: dir >= 0 ? -24 : 24, opacity: 0 }),
      }}
      initial="enter"
      animate="center"
      exit="exit"
      transition={{
        x: { type: 'spring', stiffness: 300, damping: 30 },
        opacity: { duration: 0.2 },
      }}
      style={{ width: '100%' }}
    >
      {children}
    </motion.div>
  );
}

export function Step({ children, title, tone = 'dark' }) {
  const light = tone === 'light';
  return (
    <div style={{ padding: '18px 0 8px' }}>
      {title ? (
        <h2
          style={{
            margin: '0 0 12px',
            fontFamily: font.serif,
            fontSize: 28,
            letterSpacing: '-0.02em',
            color: light ? colors.ink : colors.white,
            fontWeight: 500,
          }}
        >
          {title}
        </h2>
      ) : null}
      <div style={{ color: light ? colors.muted : 'rgba(226,232,240,0.76)', lineHeight: 1.75, fontSize: 15 }}>{children}</div>
    </div>
  );
}

function StepIndicator({ step, currentStep, onClickStep, disableStepIndicators = false, tone = 'dark' }) {
  const status = currentStep === step ? 'active' : currentStep < step ? 'inactive' : 'complete';
  const light = tone === 'light';

  return (
    <motion.div
      onClick={() => {
        if (!disableStepIndicators) onClickStep(step);
      }}
      style={{
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        cursor: disableStepIndicators ? 'default' : 'pointer',
      }}
      animate={status}
    >
      <motion.div
        variants={{
          inactive: {
            scale: 1,
            backgroundColor: light ? '#f3f4f6' : 'rgba(255,255,255,0.06)',
            color: light ? '#64748b' : 'rgba(226,232,240,0.55)',
            borderColor: light ? 'rgba(15,23,42,0.08)' : 'rgba(255,255,255,0.12)',
          },
          active: {
            scale: 1,
            backgroundColor: light ? '#ffffff' : 'rgba(15,23,42,0.9)',
            color: light ? '#0e7490' : '#a5f3fc',
            borderColor: '#06b6d4',
          },
          complete: {
            scale: 1,
            backgroundColor: '#06b6d4',
            color: '#ffffff',
            borderColor: '#06b6d4',
          },
        }}
        style={{
          width: 40,
          height: 40,
          borderRadius: 999,
          border: '2px solid',
          display: 'grid',
          placeItems: 'center',
          fontWeight: 700,
          fontFamily: font.display,
        }}
      >
        {status === 'complete' ? <Check size={18} /> : <span style={{ fontSize: 13 }}>{step}</span>}
      </motion.div>
      {status === 'active' ? (
        <motion.div
          layoutId="active-glow"
          style={{
            position: 'absolute',
            inset: -6,
            borderRadius: 999,
            background: 'rgba(6,182,212,0.22)',
            filter: 'blur(6px)',
            zIndex: -1,
          }}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        />
      ) : null}
    </motion.div>
  );
}

function StepConnector({ isComplete, tone = 'dark' }) {
  const light = tone === 'light';
  return (
    <div
      style={{
        position: 'relative',
        margin: '0 14px',
        height: 2,
        flex: 1,
        overflow: 'hidden',
        borderRadius: 999,
        background: light ? 'rgba(15,23,42,0.08)' : 'rgba(255,255,255,0.1)',
      }}
    >
      <motion.div
        style={{
          position: 'absolute',
          inset: 0,
          background: 'linear-gradient(90deg, #06b6d4, #22d3ee)',
          transformOrigin: 'left center',
        }}
        initial={{ scaleX: 0 }}
        animate={{ scaleX: isComplete ? 1 : 0 }}
        transition={{ duration: 0.45, ease: [0.33, 1, 0.68, 1] }}
      />
    </div>
  );
}

export default AnimatedStepper;
