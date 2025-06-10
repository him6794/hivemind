// HiveMind UI Enhancements - Global animations and interactions

document.addEventListener('DOMContentLoaded', function() {
    // Add smooth page transitions
    initPageTransitions();
    
    // Add interactive particle background
    initParticleBackground();
    
    // Add form input animations
    initFormAnimations();
    
    // Add loading animations
    initLoadingAnimations();
});

function initPageTransitions() {
    // Fade in animation for containers
    const containers = document.querySelectorAll('.container');
    containers.forEach((container, index) => {
        container.style.opacity = '0';
        container.style.transform = 'translateY(30px)';
        
        setTimeout(() => {
            container.style.transition = 'all 0.6s cubic-bezier(0.4, 0, 0.2, 1)';
            container.style.opacity = '1';
            container.style.transform = 'translateY(0)';
        }, index * 100);
    });
}

function initParticleBackground() {
    // Create floating particles in the background
    const particlesContainer = document.createElement('div');
    particlesContainer.className = 'particles-container';
    particlesContainer.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        width: 100%;
        height: 100%;
        pointer-events: none;
        z-index: -1;
        overflow: hidden;
    `;
    
    for (let i = 0; i < 20; i++) {
        const particle = document.createElement('div');
        particle.className = 'particle';
        particle.style.cssText = `
            position: absolute;
            width: ${Math.random() * 4 + 2}px;
            height: ${Math.random() * 4 + 2}px;
            background: radial-gradient(circle, rgba(0, 212, 255, 0.6), transparent);
            border-radius: 50%;
            left: ${Math.random() * 100}%;
            top: ${Math.random() * 100}%;
            animation: float ${Math.random() * 10 + 15}s infinite linear;
        `;
        particlesContainer.appendChild(particle);
    }
    
    document.body.appendChild(particlesContainer);
    
    // Add CSS for particle animation
    const style = document.createElement('style');
    style.textContent = `
        @keyframes float {
            0% {
                transform: translate(0, 100vh) rotate(0deg);
                opacity: 0;
            }
            10% {
                opacity: 1;
            }
            90% {
                opacity: 1;
            }
            100% {
                transform: translate(0, -100vh) rotate(360deg);
                opacity: 0;
            }
        }
    `;
    document.head.appendChild(style);
}

function initFormAnimations() {
    // Add focus animations to form inputs
    const inputs = document.querySelectorAll('input');
    inputs.forEach(input => {
        input.addEventListener('focus', function() {
            this.parentElement.style.transform = 'scale(1.02)';
            this.parentElement.style.transition = 'transform 0.3s ease';
        });
        
        input.addEventListener('blur', function() {
            this.parentElement.style.transform = 'scale(1)';
        });
        
        // Add typing effect
        input.addEventListener('input', function() {
            this.style.animation = 'inputPulse 0.3s ease';
            setTimeout(() => {
                this.style.animation = '';
            }, 300);
        });
    });
    
    // Add CSS for input animations
    const style = document.createElement('style');
    style.textContent = `
        @keyframes inputPulse {
            0% { box-shadow: 0 0 0 0 rgba(0, 212, 255, 0.4); }
            70% { box-shadow: 0 0 0 10px rgba(0, 212, 255, 0); }
            100% { box-shadow: 0 0 0 0 rgba(0, 212, 255, 0); }
        }
    `;
    document.head.appendChild(style);
}

function initLoadingAnimations() {
    // Add ripple effect to buttons
    const buttons = document.querySelectorAll('button');
    buttons.forEach(button => {
        button.addEventListener('click', function(e) {
            const ripple = document.createElement('span');
            const rect = this.getBoundingClientRect();
            const size = Math.max(rect.width, rect.height);
            const x = e.clientX - rect.left - size / 2;
            const y = e.clientY - rect.top - size / 2;
            
            ripple.style.cssText = `
                position: absolute;
                width: ${size}px;
                height: ${size}px;
                left: ${x}px;
                top: ${y}px;
                background: rgba(255, 255, 255, 0.3);
                border-radius: 50%;
                transform: scale(0);
                animation: ripple 0.6s linear;
                pointer-events: none;
            `;
            
            this.style.position = 'relative';
            this.style.overflow = 'hidden';
            this.appendChild(ripple);
            
            setTimeout(() => {
                ripple.remove();
            }, 600);
        });
    });
    
    // Add CSS for ripple animation
    const style = document.createElement('style');
    style.textContent = `
        @keyframes ripple {
            to {
                transform: scale(4);
                opacity: 0;
            }
        }
    `;
    document.head.appendChild(style);
}

// Add scroll animations
function addScrollAnimations() {
    const observer = new IntersectionObserver((entries) => {
        entries.forEach(entry => {
            if (entry.isIntersecting) {
                entry.target.style.animation = 'slideInUp 0.6s ease forwards';
            }
        });
    });
    
    document.querySelectorAll('.stat-card, .form-group, h1, h2').forEach(el => {
        observer.observe(el);
    });
}

// Initialize scroll animations after DOM is loaded
setTimeout(addScrollAnimations, 500);

// Add some global CSS improvements
const globalStyle = document.createElement('style');
globalStyle.textContent = `
    /* Smooth scrolling */
    html {
        scroll-behavior: smooth;
    }
    
    /* Selection color */
    ::selection {
        background: rgba(0, 212, 255, 0.3);
        color: var(--text-primary);
    }
    
    /* Focus outline improvements */
    *:focus {
        outline: 2px solid var(--accent-primary);
        outline-offset: 2px;
    }
    
    /* Improved button hover states */
    button:hover {
        cursor: pointer;
    }
    
    /* Link hover improvements */
    a {
        position: relative;
        overflow: hidden;
    }
    
    a::before {
        content: '';
        position: absolute;
        bottom: 0;
        left: 0;
        width: 0;
        height: 2px;
        background: var(--accent-primary);
        transition: width 0.3s ease;
    }
    
    a:hover::before {
        width: 100%;
    }
`;
document.head.appendChild(globalStyle);
