document.addEventListener('DOMContentLoaded', () => {
    const triggerBtn = document.getElementById('trigger-btn');
    const resetBtn = document.getElementById('reset-btn');
    const terminal = document.getElementById('terminal');
    const geopins = [
        document.getElementById('geopin-1'),
        document.getElementById('geopin-2'),
        document.getElementById('geopin-3')
    ];

    let isTriggered = false;
    let hzIntervals = [];

    function log(message, type = 'info') {
        const line = document.createElement('div');
        line.className = `log-line ${type}`;
        line.textContent = message;
        terminal.appendChild(line);
        terminal.scrollTop = terminal.scrollHeight;
    }

    function animateHz(element, target, duration, onComplete) {
        let start = 0;
        const startTime = performance.now();
        
        function update(currentTime) {
            const elapsed = currentTime - startTime;
            const progress = Math.min(elapsed / duration, 1);
            
            // easeOutQuart
            const ease = 1 - Math.pow(1 - progress, 4);
            const current = start + (target - start) * ease;
            
            element.textContent = current.toFixed(1);
            
            if (progress < 1) {
                requestAnimationFrame(update);
            } else if (onComplete) {
                onComplete();
            }
        }
        
        requestAnimationFrame(update);
    }

    function triggerLandslide() {
        if (isTriggered) return;
        isTriggered = true;

        log('[Network] Sending trigger command to ServerGo via RESP...', 'info');

        // Post to Rust server which bridges to ServerGo
        fetch('/trigger', { method: 'POST' })
            .then(res => res.json())
            .then(data => {
                log('[ServerGo] 0x83 MultiBandAssert / 0x87 SpatialConsensus payload forwarded.', 'success');
                simulateVibration();
            })
            .catch(err => {
                log(`[Error] Failed to connect to ServerGo proxy: ${err.message}`, 'error');
                // Simulate anyway for UI demonstration if backend is down
                simulateVibration();
            });
    }

    function simulateVibration() {
        log('[Hardware] Board B (Soldier) BOOT trigger active. Generating 20Hz shear wave...', 'warning');
        
        // Node 2 (Board B) detects first
        setTimeout(() => {
            const node2 = geopins[1];
            node2.classList.remove('safe');
            node2.classList.add('danger');
            node2.querySelector('.badge').textContent = 'HAZARD';
            node2.querySelector('.status-text').textContent = 'Vibration Detected';
            
            animateHz(node2.querySelector('.hz-value'), 20.4, 1000, () => {
                log('[VM] Node #002: SpatialConsensusAssert (0x87) Local Energy > Threshold', 'error');
                log('[Network] Node #002 broadcasting Spatial Gossip...', 'info');
                
                // Node 3 (Board C / Sim) detects slightly later
                setTimeout(() => {
                    const node3 = geopins[2];
                    node3.classList.remove('safe');
                    node3.classList.add('danger');
                    node3.querySelector('.badge').textContent = 'HAZARD';
                    node3.querySelector('.status-text').textContent = 'Vibration Detected';
                    
                    animateHz(node3.querySelector('.hz-value'), 19.8, 800, () => {
                        log('[VM] Node #003: SpatialConsensusAssert (0x87) Local Energy > Threshold', 'error');
                        log('[Consensus] 2/2 Nodes confirmed. Consensus REACHED!', 'success');
                        log('[VM] Triggering Exception 0xFE (Motor Shutdown + Alarm)', 'error');
                        
                        // Gateway Node reflects the consensus
                        setTimeout(() => {
                            const node1 = geopins[0];
                            node1.classList.remove('safe');
                            node1.classList.add('danger');
                            node1.querySelector('.badge').textContent = 'ALERT_RELAY';
                            node1.querySelector('.status-text').textContent = 'Consensus Alarm Sent';
                            
                            log('[Gateway] Forwarding Exception 0xFE to Cloud (RESP).', 'warning');
                        }, 500);
                    });
                }, 400);
            });
        }, 600);
    }

    function resetSystem() {
        isTriggered = false;
        log('[System] Resetting hardware states...', 'info');
        
        geopins.forEach((node, idx) => {
            node.classList.remove('danger');
            node.classList.add('safe');
            
            let badgeText = 'Active';
            node.querySelector('.badge').textContent = badgeText;
            node.querySelector('.status-text').textContent = 'Safe';
            
            const hzEl = node.querySelector('.hz-value');
            hzEl.textContent = '0.0';
        });
        
        log('[System] All nodes returned to Safe State.', 'success');
    }

    triggerBtn.addEventListener('click', triggerLandslide);
    resetBtn.addEventListener('click', resetSystem);
});
