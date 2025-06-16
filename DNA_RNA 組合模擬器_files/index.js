class NucleotideSimulator {
    constructor() {
        this.workCanvas = document.getElementById('work-canvas');
        this.components = [];
        this.chains = [];
        this.connections = [];
        this.draggedElement = null;
        this.componentCounter = 0;
        
        // 新增手動連線相關變數
        this.selectedComponent = null;
        this.tempLine = null;
        this.mouseMoveHandler = null;
        
        // 新增旋轉模式相關變數
        this.rotationMode = null;
        this.rotationMouseDownHandler = null;
        this.rotationMouseMoveHandler = null;
        this.rotationMouseUpHandler = null;
        this.rotationEscHandler = null;
        
        this.init();
    }
    
    init() {
        this.setupDragAndDrop();
        this.setupEventListeners();
    }
    
    setupEventListeners() {
        document.getElementById('clear-all').addEventListener('click', () => this.clearAll());
        
        // 監聽工作區域外的點擊，取消選擇組件
        document.addEventListener('click', (e) => {
            if (!this.workCanvas.contains(e.target)) {
                this.unselectComponent();
            }
        });
    }
    
    setupDragAndDrop() {
        const draggableElements = document.querySelectorAll('.component[draggable="true"]');
        draggableElements.forEach(element => {
            element.addEventListener('dragstart', (e) => this.handleDragStart(e));
        });
        
        // 設置畫布的放置
        this.workCanvas.addEventListener('dragover', (e) => this.handleDragOver(e));
        this.workCanvas.addEventListener('drop', (e) => this.handleDrop(e));
        this.workCanvas.addEventListener('dragleave', (e) => this.handleDragLeave(e));
    }
    
    handleDragStart(e) {
        this.draggedElement = e.target;
        e.dataTransfer.effectAllowed = 'copy';
    }
    
    handleDragOver(e) {
        e.preventDefault();
        e.dataTransfer.dropEffect = 'copy';
        this.workCanvas.classList.add('drag-over');
    }
    
    handleDragLeave(e) {
        this.workCanvas.classList.remove('drag-over');
    }
    
    handleDrop(e) {
        e.preventDefault();
        this.workCanvas.classList.remove('drag-over');
        if (!this.draggedElement) return;
        
        const rect = this.workCanvas.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;
        
        this.createComponent(this.draggedElement, x, y);
        this.draggedElement = null;
    }
    
    createComponent(originalElement, x, y) {
        if (!originalElement || typeof originalElement.cloneNode !== 'function') {
            console.error("無法創建組件：originalElement 無效", originalElement);
            return;
        }
        const component = originalElement.cloneNode(true);
        if (!component || typeof component.style === 'undefined') {
            console.error("克隆組件失敗或克隆結果無效", component);
            return;
        }

        component.id = `component-${this.componentCounter++}`;
        component.style.position = 'absolute';
        component.style.left = `${x - 30}px`;
        component.style.top = `${y - 20}px`;
        component.style.cursor = 'move';
        component.draggable = false;
        component.classList.add('canvas-component');

        // 初始化 CSS 自定義屬性用於變換
        component.style.setProperty('--component-rotate', '0deg');
        component.style.setProperty('--component-scale', '1');
        
        // 複製數據屬性
        const type = originalElement.dataset.type;
        const base = originalElement.dataset.base;
        const sugarType = originalElement.dataset.sugarType;
        
        component.dataset.type = type;
        if (base) component.dataset.base = base;
        if (sugarType) component.dataset.sugarType = sugarType;
        
        // 創建組件數據對象
        const componentData = {
            element: component,
            id: component.id,
            type: type,
            base: base,
            sugarType: sugarType,
            x: x - 30,
            y: y - 20,
            inChain: false,
            chainId: null,
            connections: [],
            rotation: 0, // 新增 rotation 屬性
        };
        
        // 綁定事件
        component.addEventListener('click', (e) => {
            e.stopPropagation();
            this.handleComponentClick(component);
        });
        component.addEventListener('contextmenu', (e) => { // 右鍵點擊事件
            e.stopPropagation();
            this.handleComponentRightClick(e, componentData);
        });
        
        // 添加移動功能
        this.makeDraggable(component);
        
        // 添加到畫布和組件列表
        this.workCanvas.appendChild(component);
        this.components.push(componentData);
        
        this.removeCanvasHint();
    }
    
    handleComponentClick(element) {
        const componentData = this.components.find(c => c.element === element);
        if (!componentData) return;
        
        console.log("Component clicked:", componentData.type, componentData.id);
        
        if (this.selectedComponent && this.selectedComponent.id === componentData.id) {
            console.log("Unselecting component");
            this.unselectComponent();
            return;
        }
        
        if (this.selectedComponent) {
            console.log("Attempting to connect with previously selected component");
            const sourceComponentData = this.selectedComponent;
            this.unselectComponent();
            
            if (sourceComponentData) {
                this.tryConnectComponents(sourceComponentData, componentData);
            }        } else {
            console.log("Selecting component for connection");
            
            // 如果在旋轉模式，先退出旋轉模式
            if (this.rotationMode) {
                this.stopSmoothRotation();
            }
              this.selectedComponent = componentData;
            element.classList.add('selected');
            element.style.setProperty('--component-scale', '1.1');
            this.createTempLine(componentData);
            
            this.updateModeIndicator('connection', '連線模式');
            this.showMessage("選擇目標組件以連線，或按ESC取消", "info");
        }
    }    handleComponentRightClick(event, clickedComponentData) {
        event.preventDefault();
        if (!clickedComponentData) return;

        // 開始連續旋轉模式
        this.startSmoothRotation(clickedComponentData);
    }    startSmoothRotation(centerComponent) {
        // 如果已經在旋轉模式，先停止
        if (this.rotationMode) {
            this.stopSmoothRotation();
        }

        // 如果在連線模式，先退出連線模式
        if (this.selectedComponent) {
            this.unselectComponent();
        }        this.rotationMode = {
            centerComponent: centerComponent,
            isRotating: false,
            startAngle: 0,
            totalRotation: 0,
            chainComponents: [],
            initialPositions: new Map() // 記錄組件的初始位置
        };        // 獲取所有連接的組件
        if (centerComponent.inChain && centerComponent.chainId) {
            this.rotationMode.chainComponents = this.components.filter(comp => 
                comp.chainId === centerComponent.chainId
            );
        } else {
            // 如果沒有在鏈中，檢查是否有直接連接的組件
            const connectedComponents = [centerComponent];
            this.findConnectedComponents(centerComponent, connectedComponents);
            this.rotationMode.chainComponents = connectedComponents;
        }

        // 記錄所有組件的初始位置和角度
        const centerX = centerComponent.x + 30;
        const centerY = centerComponent.y + 30;
        
        this.rotationMode.chainComponents.forEach(comp => {
            this.rotationMode.initialPositions.set(comp.id, {
                x: comp.x,
                y: comp.y,
                rotation: comp.rotation,
                relativeX: comp.x + 30 - centerX,
                relativeY: comp.y + 30 - centerY,
                initialAngle: Math.atan2(comp.y + 30 - centerY, comp.x + 30 - centerX)
            });
        });

        // 高亮顯示旋轉模式
        this.rotationMode.chainComponents.forEach(comp => {
            comp.element.classList.add('rotation-mode');
        });

        // 添加旋轉控制事件
        this.addRotationControls(centerComponent);        // 顯示提示信息
        this.updateModeIndicator('rotation', '旋轉模式');
        this.showMessage("進入旋轉模式：左鍵拖動旋轉，按住Shift精細調整，右鍵或ESC退出", "info");
    }

    addRotationControls(centerComponent) {
        const centerX = centerComponent.x + 30;
        const centerY = centerComponent.y + 30;

        // 鼠標按下開始旋轉
        this.rotationMouseDownHandler = (e) => {
            if (e.button === 0) { // 左鍵
                e.preventDefault();
                this.rotationMode.isRotating = true;
                
                const canvasRect = this.workCanvas.getBoundingClientRect();
                const mouseX = e.clientX - canvasRect.left;
                const mouseY = e.clientY - canvasRect.top;
                
                this.rotationMode.startAngle = Math.atan2(mouseY - centerY, mouseX - centerX);
                document.body.style.cursor = 'grabbing';
            } else if (e.button === 2) { // 右鍵退出
                e.preventDefault();
                this.stopSmoothRotation();
            }
        };        // 鼠標移動進行旋轉
        this.rotationMouseMoveHandler = (e) => {
            if (!this.rotationMode.isRotating) return;

            const canvasRect = this.workCanvas.getBoundingClientRect();
            const mouseX = e.clientX - canvasRect.left;
            const mouseY = e.clientY - canvasRect.top;
            
            // 計算當前滑鼠相對於中心點的角度
            const currentAngle = Math.atan2(mouseY - centerY, mouseX - centerX);
            
            // 計算與起始角度的差值
            let deltaAngle = currentAngle - this.rotationMode.startAngle;
            
            // 處理角度跨越問題 (-π 到 π)
            if (deltaAngle > Math.PI) {
                deltaAngle -= 2 * Math.PI;
            } else if (deltaAngle < -Math.PI) {
                deltaAngle += 2 * Math.PI;
            }
            
            // 檢查是否按住 Shift 鍵進行精細調整
            const fineTuning = e.shiftKey;
            const rotationFactor = fineTuning ? 0.3 : 1;
            
            // 轉換為度數並應用旋轉
            const deltaAngleDegrees = deltaAngle * 180 / Math.PI * rotationFactor;
            
            // 應用旋轉（這會更新所有組件的絕對旋轉角度）
            this.applyAbsoluteRotationToChain(centerComponent, currentAngle * 180 / Math.PI * rotationFactor);
            
            // 更新模式指示器顯示精細調整狀態
            const modeText = fineTuning ? '旋轉模式 (精細調整)' : '旋轉模式';
            this.updateModeIndicator('rotation', modeText);
        };

        // 鼠標抬起停止旋轉
        this.rotationMouseUpHandler = (e) => {
            if (e.button === 0) {
                this.rotationMode.isRotating = false;
                document.body.style.cursor = 'default';
            }
        };

        // ESC鍵退出
        this.rotationEscHandler = (e) => {
            if (e.key === 'Escape') {
                this.stopSmoothRotation();
            }
        };

        // 添加事件監聽器
        document.addEventListener('mousedown', this.rotationMouseDownHandler);
        document.addEventListener('mousemove', this.rotationMouseMoveHandler);
        document.addEventListener('mouseup', this.rotationMouseUpHandler);
        document.addEventListener('keydown', this.rotationEscHandler);
        document.addEventListener('contextmenu', (e) => e.preventDefault());
    }

    applyRotationToChain(centerComponent, deltaAngle) {
        const centerX = centerComponent.x + 30;
        const centerY = centerComponent.y + 30;

        this.rotationMode.chainComponents.forEach(comp => {
            if (comp.id === centerComponent.id) {
                // 中心組件只更新自身旋轉
                comp.rotation = (comp.rotation + deltaAngle) % 360;
            } else {
                // 其他組件圍繞中心點旋轉
                const relativeX = comp.x + 30 - centerX;
                const relativeY = comp.y + 30 - centerY;
                
                const rotatedPos = this.rotatePoint(relativeX, relativeY, deltaAngle);
                
                comp.x = centerX + rotatedPos.x - 30;
                comp.y = centerY + rotatedPos.y - 30;
                comp.element.style.left = `${comp.x}px`;
                comp.element.style.top = `${comp.y}px`;
                
                comp.rotation = (comp.rotation + deltaAngle) % 360;
            }
            
            comp.element.style.setProperty('--component-rotate', comp.rotation + 'deg');
        });

        this.updateConnections();
    }

    stopSmoothRotation() {
        if (!this.rotationMode) return;

        // 移除高亮
        this.rotationMode.chainComponents.forEach(comp => {
            comp.element.classList.remove('rotation-mode');
        });

        // 移除事件監聽器
        if (this.rotationMouseDownHandler) {
            document.removeEventListener('mousedown', this.rotationMouseDownHandler);
            document.removeEventListener('mousemove', this.rotationMouseMoveHandler);
            document.removeEventListener('mouseup', this.rotationMouseUpHandler);
            document.removeEventListener('keydown', this.rotationEscHandler);
        }        document.body.style.cursor = 'default';
        this.rotationMode = null;
        
        this.updateModeIndicator(null, null);
        this.showMessage("", ""); // 清除提示信息
    }
    
    // 新增旋轉點的輔助函數
    rotatePoint(x, y, angle) {
        const radians = (angle * Math.PI) / 180;
        const cos = Math.cos(radians);
        const sin = Math.sin(radians);
        
        return {
            x: x * cos - y * sin,
            y: x * sin + y * cos
        };
    }
    
    makeDraggable(element) {
        let isDragging = false;
        let startX, startY;
        let initialLeft, initialTop;
        let chainComponents = [];
        let dragStartTime = 0;
        let lastClickTime = 0;
          // 移動處理函數
        const moveHandler = (e) => {
            if (this.selectedComponent || !isDragging) return;
            
            const component = this.components.find(c => c.id === element.id);
            if (!component) return;
            
            const deltaX = e.clientX - startX;
            const deltaY = e.clientY - startY;
            
            // 使用改進的位置更新函數
            this.updateAllConnectedComponentsPosition(component, deltaX, deltaY, chainComponents);
        };
        
        // 結束處理函數
        const endHandler = () => {
            if (isDragging) {
                isDragging = false;
                element.classList.remove('dragging');
                
                chainComponents.forEach(comp => {
                    delete comp._initialX;
                    delete comp._initialY;
                });
                
                const component = this.components.find(c => c.id === element.id);
                if (component) {
                    const rect = this.workCanvas.getBoundingClientRect();
                    const isOutside = 
                        component.x < 0 || 
                        component.y < 0 || 
                        component.x > rect.width - 60 || 
                        component.y > rect.height - 60;
                    
                    if (isOutside) {
                        if (component.inChain) {
                            this.removeFromChain(component);
                        }
                        this.removeComponent(component);
                    } else {
                        this.checkAutoAssembly();
                    }
                }
                
                chainComponents = [];
            } 
            // 如果不是拖動且時間短，視為點擊
            else if ((Date.now() - dragStartTime < 200)) {
                this.handleComponentClick(element);
            }
        };

        element.addEventListener('mousedown', (e) => {
            if (e.button !== 0) return;
            if (this.selectedComponent) return;
            
            e.preventDefault();
            
            isDragging = false;
            dragStartTime = Date.now();
            
            startX = e.clientX;
            startY = e.clientY;
            initialLeft = parseInt(element.style.left);
            initialTop = parseInt(element.style.top);
            
            const now = Date.now();
            if (now - lastClickTime < 300) {
                console.log("Double click detected!");
                this.handleDoubleClick(element);
                lastClickTime = 0;
                return;
            }
            lastClickTime = now;
              const component = this.components.find(c => c.id === element.id);
            if (!component) return;
            
            // 獲取所有需要一起移動的組件
            if (component.inChain) {
                chainComponents = this.components.filter(c => c.chainId === component.chainId);
            } else {
                // 如果沒有在鏈中，查找所有連接的組件
                chainComponents = [];
                this.findConnectedComponents(component, chainComponents);
            }
            
            // 記錄所有組件的初始位置
            chainComponents.forEach(comp => {
                comp._initialX = comp.x;
                comp._initialY = comp.y;
            });
            
            isDragging = true;
            element.classList.add('dragging');
            
            document.addEventListener('mousemove', moveHandler);
            document.addEventListener('mouseup', endHandler);
        });
        
        // 防止點擊事件與拖動衝突
        element.addEventListener('click', (e) => {
            if (isDragging) {
                e.stopPropagation();
                e.preventDefault();
            }
        });
    }

    unselectComponent() {
        if (this.selectedComponent) {
            const element = this.selectedComponent.element;
            if (element) {
                element.classList.remove('selected');
                element.style.setProperty('--component-scale', '1');            }
            this.selectedComponent = null;
        }
        this.removeTempLine();
        this.updateModeIndicator(null, null);
    }
    
    // 添加觸控版本的臨時連線
    createTempLine(component) {
        this.removeTempLine();
        
        this.tempLine = document.createElement('div');
        this.tempLine.className = 'connection-line temp-line';
        this.workCanvas.appendChild(this.tempLine);
        
        const x1 = component.x + 30;
        const y1 = component.y + 30;
        
        // 桌面設備滑鼠跟隨功能
        this.mouseMoveHandler = (e) => {
            const canvasRect = this.workCanvas.getBoundingClientRect();
            const mouseX = Math.max(0, Math.min(e.clientX - canvasRect.left, canvasRect.width));
            const mouseY = Math.max(0, Math.min(e.clientY - canvasRect.top, canvasRect.height));
            
            const length = Math.sqrt((mouseX - x1) ** 2 + (mouseY - y1) ** 2);
            const angle = Math.atan2(mouseY - y1, mouseX - x1) * 180 / Math.PI;
            
            this.tempLine.style.left = `${x1}px`;
            this.tempLine.style.top = `${y1}px`;
            this.tempLine.style.width = `${length}px`;
            this.tempLine.style.transform = `rotate(${angle}deg)`;
        };
        
        document.addEventListener('mousemove', this.mouseMoveHandler);
        
        this.escKeyHandler = (e) => {
            if (e.key === 'Escape') {
                this.unselectComponent();
            }
        };
        document.addEventListener('keydown', this.escKeyHandler);
        
        document.body.style.cursor = 'crosshair';
        this.workCanvas.classList.add('connection-mode');
    }

    // 修改移除組件函數以支援觸控
    removeComponent(component) {
        // 移除所有相關連線
        if (component.connections.length > 0) {
            const connectionsCopy = [...component.connections];
            connectionsCopy.forEach(conn => {
                const otherComp = this.components.find(c => c.id === conn.to);
                if (otherComp) {
                    otherComp.connections = otherComp.connections.filter(c => c.to !== component.id);
                }
                if (conn.element && conn.element.parentNode) {
                    conn.element.parentNode.removeChild(conn.element);
                }
            });
        }
        
        // 從DOM移除
        if (component.element.parentNode) {
            component.element.parentNode.removeChild(component.element);
        }
        
        // 從組件列表移除
        this.components = this.components.filter(c => c.id !== component.id);
        
        // 重新計算鏈
        if (component.inChain) {
            this.recalculateChain(component.chainId);
        }
    }
      // 修正連接核苷酸函數
    connectNucleotides(comp1, comp2, connection) {
        console.log(`連線組件: ${comp1.id} (${comp1.type}) <-> ${comp2.id} (${comp2.type})`);
        
        // 為兩個組件建立雙向連接
        const conn1 = {
            to: comp2.id,
            element: connection,
            type: 'direct'
        };
        
        const conn2 = {
            to: comp1.id,
            element: connection,
            type: 'reverse'
        };
        
        // 確保connections陣列存在
        if (!comp1.connections) comp1.connections = [];
        if (!comp2.connections) comp2.connections = [];
        
        // 添加連接（避免重複）
        if (!comp1.connections.some(c => c.to === comp2.id)) {
            comp1.connections.push(conn1);
        }
        if (!comp2.connections.some(c => c.to === comp1.id)) {
            comp2.connections.push(conn2);
        }
        
        // 如果組件不在鏈中，創建或加入鏈
        this.connectComponents(comp1, comp2);
        
        console.log(`成功建立連接: ${comp1.id} <-> ${comp2.id}`);
    }
    
    // 修復處理雙擊事件
    handleDoubleClick(element) {
        const component = this.components.find(c => c.id === element.id);
        if (!component) return;
        
        console.log("處理雙擊事件，移除所有連線:", component.id);
        
        // 記錄是否在鏈中以及鏈ID，因為連線移除後這些信息會丟失
        const wasInChain = component.inChain;
        const chainId = component.chainId;
        
        // 移除此組件的所有連線
        if (component.connections.length > 0) {
            // 創建連線副本，因為我們會修改原陣列
            const connectionsCopy = [...component.connections];
            
            // 逐一移除連線
            connectionsCopy.forEach(conn => {
                // 獲取另一端組件
                const otherComp = this.components.find(c => c.id === conn.to);
                if (!otherComp) return;
                
                console.log(`移除連線: ${component.id} -> ${otherComp.id}`);
                
                // 從DOM中移除連線元素
                if (conn.element && conn.element.parentNode) {
                    conn.element.parentNode.removeChild(conn.element);
                }
                
                // 從另一端組件的連線列表中移除
                otherComp.connections = otherComp.connections.filter(c => c.to !== component.id);
            });
            
            // 清空當前組件的連線
            component.connections = [];
            
            // 重置組件的鏈狀態
            component.inChain = false;
            component.chainId = null;
            component.element.classList.remove('in-chain', 'complete-chain', 'incomplete-chain', 'paired');
            
            // 如果組件之前在鏈中，重新計算鏈
            if (wasInChain) {
                this.recalculateChain(chainId);
            }
            
            this.showMessage(`已移除 ${component.type} 的所有連線`, 'info');
        }
    }
    
    // 新增重新計算特定鏈的函數
    recalculateChain(chainId) {
        // 尋找該鏈中的所有組件
        const chainComponents = this.components.filter(c => c.chainId === chainId);
        
        // 如果鏈中沒有組件了，移除鏈
        if (chainComponents.length === 0) {
            this.chains = this.chains.filter(c => c.id !== chainId);
            return;
        }
        
        // 如果只剩一個組件，解除其鏈狀態
        if (chainComponents.length === 1) {
            const comp = chainComponents[0];
            comp.inChain = false;
            comp.chainId = null;
            comp.element.classList.remove('in-chain', 'complete-chain', 'incomplete-chain', 'paired');
            this.chains = this.chains.filter(c => c.id !== chainId);
            return;
        }
        
        // 更新鏈狀態
        const chain = this.chains.find(c => c.id === chainId);
        if (chain) {
            chain.components = chainComponents;
            this.updateChainStatus(chain);
        }
    }
    
    removeCanvasHint() {
        const hint = this.workCanvas.querySelector('.canvas-hint');
        if (hint && this.components.length > 0) {
            hint.style.display = 'none';
        }
    }
    
    checkAutoAssembly() {
        // 檢查是否有組件靠近可以自動組合
        for (let i = 0; i < this.components.length; i++) {
            for (let j = i + 1; j < this.components.length; j++) {
                const comp1 = this.components[i];
                const comp2 = this.components[j];
                
                if (this.isNearby(comp1, comp2) && 
                    this.validateConnection(comp1, comp2) && 
                    !this.areConnected(comp1, comp2)) {
                    const conn = this.createConnectionElement(comp1, comp2);
                    this.connectNucleotides(comp1, comp2, conn);
                }
            }
        }
    }
    
    areConnected(comp1, comp2) {
        return comp1.connections.some(c => c.to === comp2.id);
    }
    
    isNearby(comp1, comp2, threshold = 50) {
        const dx = comp1.x - comp2.x;
        const dy = comp1.y - comp2.y;
        return Math.sqrt(dx * dx + dy * dy) < threshold;
    }
    
    canConnect(comp1, comp2) {
        // 檢查是否可以連接組件
        if (comp1.inChain && comp2.inChain && comp1.chainId === comp2.chainId) {
            return false; // 已經在同一條鏈中
        }
        
        // 核苷酸組件可以連接（磷酸基、核糖、鹼基）
        const nucleotideTypes = ['phosphate', 'sugar', 'base'];
        return nucleotideTypes.includes(comp1.type) && nucleotideTypes.includes(comp2.type);
    }
    
    connectComponents(comp1, comp2) {
        // 如果都不在鏈中，創建新鏈
        if (!comp1.inChain && !comp2.inChain) {
            const chainId = `chain-${Date.now()}`;
            this.createChain(chainId, [comp1, comp2]);
        }
        // 如果一個在鏈中，將另一個加入
        else if (comp1.inChain && !comp2.inChain) {
            this.addToChain(comp1.chainId, comp2);
        }
        else if (!comp1.inChain && comp2.inChain) {
            this.addToChain(comp2.chainId, comp1);
        }
        // 如果都在不同鏈中，合併鏈
        else if (comp1.chainId !== comp2.chainId) {
            this.mergeChains(comp1.chainId, comp2.chainId);
        }
    }
    
    createChain(chainId, components) {
        const chain = {
            id: chainId,
            components: components,
            isComplete: false
        };
        
        components.forEach(comp => {
            comp.inChain = true;
            comp.chainId = chainId;
            comp.element.classList.add('in-chain');
        });
        
        this.chains.push(chain);
        this.updateChainStatus(chain);
    }
    
    addToChain(chainId, component) {
        const chain = this.chains.find(c => c.id === chainId);
        if (chain) {
            chain.components.push(component);
            component.inChain = true;
            component.chainId = chainId;
            component.element.classList.add('in-chain');
            this.updateChainStatus(chain);
        }
    }
    
    mergeChains(chainId1, chainId2) {
        const chain1 = this.chains.find(c => c.id === chainId1);
        const chain2 = this.chains.find(c => c.id === chainId2);
        
        if (chain1 && chain2) {
            // 將 chain2 的組件合併到 chain1
            chain2.components.forEach(comp => {
                comp.chainId = chainId1;
                chain1.components.push(comp);
            });
            
            // 移除 chain2
            this.chains = this.chains.filter(c => c.id !== chainId2);
            this.updateChainStatus(chain1);
        }
    }
    
    updateChainStatus(chain) {
        // 檢查鏈是否完整（包含磷酸基、核糖、鹼基）
        const hasPhosphate = chain.components.some(c => c.type === 'phosphate');
        const hasSugar = chain.components.some(c => c.type === 'sugar');
        const hasBase = chain.components.some(c => c.type === 'base');
        
        chain.isComplete = hasPhosphate && hasSugar && hasBase;
        
        // 更新視覺狀態
        chain.components.forEach(comp => {
            if (chain.isComplete) {
                comp.element.classList.add('complete-chain');
                comp.element.classList.remove('incomplete-chain');
            } else {
                comp.element.classList.add('incomplete-chain');
                comp.element.classList.remove('complete-chain');
            }
        });
    }
    
    validatePairing() {
        // 清除之前的配對
        this.clearConnections();
        
        const completeChains = this.chains.filter(chain => chain.isComplete);
        
        if (completeChains.length < 2) {
            this.showMessage('需要至少兩條完整的核苷酸鏈才能進行配對', 'error');
            return;
        }
        
        let pairingsFound = 0;
        
        // 檢查所有完整鏈的配對
        for (let i = 0; i < completeChains.length; i++) {
            for (let j = i + 1; j < completeChains.length; j++) {
                const chain1 = completeChains[i];
                const chain2 = completeChains[j];
                
                if (this.canChainsPair(chain1, chain2)) {
                    this.createConnection(chain1, chain2);
                    pairingsFound++;
                }
            }
        }
        
        if (pairingsFound > 0) {
            this.showMessage(`找到 ${pairingsFound} 個正確的鏈配對！`, 'success');
        } else {
            this.showMessage('沒有找到正確的配對。檢查鹼基配對規則：A-T, A-U, C-G', 'error');
        }
    }
    
    canChainsPair(chain1, chain2) {
        const base1 = this.getChainBase(chain1);
        const base2 = this.getChainBase(chain2);
        
        if (!base1 || !base2) return false;
        
        // 檢查配對規則
        const validPairs = {
            'A': ['T', 'U'],
            'T': ['A'],
            'U': ['A'],
            'C': ['G'],
            'G': ['C']
        };
        
        return validPairs[base1] && validPairs[base1].includes(base2);
    }
    
    getChainBase(chain) {
        const baseComponent = chain.components.find(c => c.type === 'base');
        return baseComponent ? baseComponent.base : null;
    }
    
    // 改進的連線創建函數，包含鍵類型检测
    createConnection(comp1, comp2) {
        console.log(`創建連線: ${comp1.id} -> ${comp2.id}`);
        
        const connection = document.createElement('div');
        connection.className = 'connection';
        
        // 根據組件類型決定連線樣式
        const bondType = this.determineBondType(comp1, comp2);
        connection.classList.add(bondType);
        
        // 設置連線位置
        const x1 = comp1.x + 30;
        const y1 = comp1.y + 30;
        const x2 = comp2.x + 30;
        const y2 = comp2.y + 30;
        
        const length = Math.sqrt((x2 - x1) ** 2 + (y2 - y1) ** 2);
        const angle = Math.atan2(y2 - y1, x2 - x1) * 180 / Math.PI;
        
        connection.style.position = 'absolute';
        connection.style.left = `${x1}px`;
        connection.style.top = `${y1}px`;
        connection.style.width = `${length}px`;
        connection.style.height = '2px';
        connection.style.transformOrigin = '0 50%';
        connection.style.transform = `rotate(${angle}deg)`;
        connection.style.zIndex = '10';
        
        // 添加連線懸停效果
        connection.addEventListener('mouseenter', () => {
            this.showConnectionInfo(comp1, comp2, bondType);
        });
        
        connection.addEventListener('mouseleave', () => {
            this.hideConnectionInfo();
        });
        
        // 添加連線右鍵刪除
        connection.addEventListener('contextmenu', (e) => {
            e.preventDefault();
            this.removeConnection(comp1, comp2, connection);
        });
        
        this.canvas.appendChild(connection);
        return connection;
    }
    
    // 判斷化學鍵類型
    determineBondType(comp1, comp2) {
        // 氫鍵：鹼基之間的配對
        if (comp1.type === 'base' && comp2.type === 'base') {
            return 'hydrogen-bond';
        }
        
        // 強鍵：磷酸基與糖類之間
        if ((comp1.type === 'phosphate' && comp2.type === 'sugar') ||
            (comp1.type === 'sugar' && comp2.type === 'phosphate')) {
            return 'strong-bond';
        }
        
        // 弱鍵：糖類與鹼基之間
        if ((comp1.type === 'sugar' && comp2.type === 'base') ||
            (comp1.type === 'base' && comp2.type === 'sugar')) {
            return 'weak-bond';
        }
        
        return 'strong-bond'; // 默認
    }
    
    // 顯示連線信息
    showConnectionInfo(comp1, comp2, bondType) {
        const info = document.createElement('div');
        info.className = 'connection-info';
        info.innerHTML = `
            <div class="bond-type">${this.getBondTypeName(bondType)}</div>
            <div class="components">${comp1.id} ↔ ${comp2.id}</div>
        `;
        info.style.cssText = `
            position: fixed;
            background: rgba(0, 0, 0, 0.8);
            color: white;
            padding: 8px 12px;
            border-radius: 4px;
            font-size: 12px;
            z-index: 1000;
            pointer-events: none;
            left: ${event.clientX + 10}px;
            top: ${event.clientY - 30}px;
        `;
        document.body.appendChild(info);
        this.connectionInfoElement = info;
    }
    
    // 隱藏連線信息
    hideConnectionInfo() {
        if (this.connectionInfoElement) {
            document.body.removeChild(this.connectionInfoElement);
            this.connectionInfoElement = null;
        }
    }
    
    // 獲取鍵類型名稱
    getBondTypeName(bondType) {
        const names = {
            'hydrogen-bond': '氫鍵',
            'strong-bond': '共價鍵',
            'weak-bond': '糖苷鍵'
        };
        return names[bondType] || '化學鍵';
    }
    
    clearConnections() {
        // 移除連接線
        this.connections.forEach(conn => {
            if (conn.line.parentNode) {
                conn.line.parentNode.removeChild(conn.line);
            }
        });
        this.connections = [];
        
        // 移除配對狀態
        this.components.forEach(comp => {
            comp.element.classList.remove('paired');
        });
    }
    
    // 清理孤立的連線引用
    cleanupConnections() {
        this.components.forEach(comp => {
            if (comp.connections) {
                comp.connections = comp.connections.filter(conn => {
                    const targetComp = this.components.find(c => c.id === conn.to);
                    const isValid = targetComp && conn.element && conn.element.parentNode;
                    
                    // 如果連線無效，移除DOM元素
                    if (!isValid && conn.element && conn.element.parentNode) {
                        conn.element.parentNode.removeChild(conn.element);
                    }
                    
                    return isValid;
                });
            }
        });
    }

    // 改進的移除鏈函數
    removeFromChain(component) {
        if (!component.inChain || !component.chainId) return;
        
        const chainId = component.chainId;
        
        // 從鏈中移除組件
        component.inChain = false;
        component.chainId = null;
        component.element.classList.remove('in-chain', 'complete-chain', 'incomplete-chain');
        
        // 重新計算鏈
        this.recalculateChain(chainId);
    }

    clearAll() {
        // 清除所有組件
        this.components.forEach(comp => {
            if (comp.element.parentNode) {
                comp.element.parentNode.removeChild(comp.element);
            }
        });
        
        // 清除所有連線
        const connections = document.querySelectorAll('.connection-line');
        connections.forEach(conn => {
            if (conn.parentNode) {
                conn.parentNode.removeChild(conn);
            }
        });
        
        // 清除臨時線
        this.removeTempLine();
        this.unselectComponent();
        
        // 重置狀態
        this.components = [];
        this.chains = [];
        this.connections = [];
        this.componentCounter = 0;
        
        // 恢復提示
        const hint = this.workCanvas.querySelector('.canvas-hint');
        if (hint) {
            hint.style.display = 'block';
        }
        
        this.showMessage('已清除所有組件');
    }
    
    showMessage(message, type = 'info') {
        const resultDiv = document.getElementById('validation-result');
        resultDiv.textContent = message;
        resultDiv.className = `validation-result ${type}`;
        
        // 3秒後清除消息
        setTimeout(() => {
            resultDiv.textContent = '';
            resultDiv.className = 'validation-result';
        }, 3000);
    }
    
    // 模式指示器管理
    updateModeIndicator(mode, text) {
        const indicator = document.getElementById('mode-indicator');
        if (!indicator) return;
        
        indicator.className = 'mode-indicator';
        
        if (mode && text) {
            indicator.textContent = text;
            indicator.classList.add('show', mode + '-mode');
        } else {
            indicator.classList.remove('show');
            setTimeout(() => {
                indicator.textContent = '';
            }, 300);
        }
    }

    // 需要添加的函數，用於嘗試連線兩個組件
    tryConnectComponents(comp1, comp2) {
        console.log(`嘗試連接 ${comp1.type} 和 ${comp2.type}`);
        
        // 檢查這兩個組件是否可以連接
        const canConnect = this.validateConnection(comp1, comp2);
        console.log("驗證連接結果:", canConnect);
        
        if (canConnect) {
            // 建立連線
            const connection = this.createConnectionElement(comp1, comp2);
            this.connectNucleotides(comp1, comp2, connection);
            this.showMessage(`成功連接 ${comp1.type} 和 ${comp2.type}`, 'success');
        } else {
            // 建立臨時的無效連線（紅色）
            const invalidConnection = this.createConnectionElement(comp1, comp2, true);
            
            // 3秒後移除無效連線
            setTimeout(() => {
                if (invalidConnection && invalidConnection.parentNode) {
                    invalidConnection.parentNode.removeChild(invalidConnection);
                }
            }, 3000);
            
            this.showMessage(`${comp1.type} 無法與 ${comp2.type} 直接連接`, 'error');
        }
    }

    // 需要添加的函數，用於創建連線元素
    createConnectionElement(comp1, comp2, isInvalid = false) {
        const line = document.createElement('div');
        line.className = isInvalid ? 'connection-line invalid' : 'connection-line';
        
        // 計算兩個組件的中心點坐標
        const x1 = comp1.x + 30; // 組件中心點 X
        const y1 = comp1.y + 30; // 組件中心點 Y
        const x2 = comp2.x + 30; // 目標組件中心點 X
        const y2 = comp2.y + 30; // 目標組件中心點 Y
        
        // 計算連線長度和角度
        const length = Math.sqrt((x2 - x1) ** 2 + (y2 - y1) ** 2);
        const angle = Math.atan2(y2 - y1, x2 - x1) * 180 / Math.PI;
        
        // 設置連線樣式
        line.style.position = 'absolute';
        line.style.left = `${x1}px`;
        line.style.top = `${y1}px`;
        line.style.width = `${length}px`;
        line.style.transformOrigin = '0 50%';
        line.style.transform = `rotate(${angle}deg)`;
        line.style.zIndex = isInvalid ? '16' : '10';
        
        // 儲存連線的組件ID
        line.dataset.fromId = comp1.id;
        line.dataset.toId = comp2.id;
        
        this.workCanvas.appendChild(line);
        
        return line;
    }

    removeTempLine() {
        if (this.tempLine && this.tempLine.parentNode) {
            this.tempLine.parentNode.removeChild(this.tempLine);
            this.tempLine = null;
        }
        
        // 確保移除所有相關事件監聽器
        if (this.mouseMoveHandler) {
            document.removeEventListener('mousemove', this.mouseMoveHandler);
            this.mouseMoveHandler = null;
        }
        
        if (this.escKeyHandler) {
            document.removeEventListener('keydown', this.escKeyHandler);
            this.escKeyHandler = null;
        }
        
        // 恢復正常模式
        document.body.style.cursor = '';
        this.workCanvas.classList.remove('connection-mode');
    }

    // 添加組件間連線驗證功能
    validateConnection(comp1, comp2) {
        console.log("驗證連接:", comp1.type, comp2.type);
        
        // 檢查是否已經連接
        if (this.areConnected(comp1, comp2)) {
            console.log("已經連接");
            return false;
        }
        
        // 如果兩個都是鹼基，檢查配對規則
        if (comp1.type === 'base' && comp2.type === 'base') {
            console.log("檢查鹼基配對規則:", comp1.base, comp2.base);
            
            // 定義鹼基配對規則
            const validPairs = {
                'A': ['T', 'U'],
                'T': ['A'],
                'U': ['A'],
                'C': ['G'],
                'G': ['C']
            };
            
            const isValidPair = validPairs[comp1.base] && validPairs[comp1.base].includes(comp2.base);
            console.log("鹼基配對結果:", isValidPair);
            return isValidPair;
        }
        
        // 檢查是否是相同類型（除了鹼基外不能連接）
        if (comp1.type === comp2.type && comp1.type !== 'base') {
            console.log("相同類型不能連接");
            return false;
        }
        
        // 磷酸基只能連接核糖
        if (comp1.type === 'phosphate' && comp2.type !== 'sugar') {
            console.log("磷酸基只能連接核糖");
            return false;
        }
        if (comp2.type === 'phosphate' && comp1.type !== 'sugar') {
            console.log("磷酸基只能連接核糖");
            return false;
        }
        
        // 檢查DNA/RNA特定規則
        if (comp1.type === 'sugar' && comp2.type === 'base') {
            if (comp1.sugarType === 'deoxyribose' && comp2.base === 'U') {
                console.log("去氧核醣不能連接尿嘧啶");
                return false; // 去氧核醣不能與U連接
            }
            if (comp1.sugarType === 'ribose' && comp2.base === 'T') {
                console.log("核醣不能連接胸腺嘧啶");
                return false; // 核醣不能與T連接
            }
        }
        
        if (comp2.type === 'sugar' && comp1.type === 'base') {
            if (comp2.sugarType === 'deoxyribose' && comp1.base === 'U') {
                console.log("去氧核醣不能連接尿嘧啶");
                return false; // 去氧核醣不能與U連接
            }
            if (comp2.sugarType === 'ribose' && comp1.base === 'T') {
                console.log("核醣不能連接胸腺嘧啶");
                return false; // 核醣不能與T連接
            }
        }
        
        console.log("連接驗證通過");
        return true;
    }    // 將 updateConnections 函數搬到前面，確保它在被調用前已被定義
    updateConnections() {
        try {
            console.log("更新連線位置");
            
            // 首先清理無效的連線
            this.cleanupConnections();
            
            // 更新所有連線的位置
            for (const comp of this.components) {
                if (!comp.connections || !Array.isArray(comp.connections)) continue;
                
                // 過濾掉無效的連線
                comp.connections = comp.connections.filter(conn => {
                    // 檢查連線目標是否存在
                    const toComp = this.components.find(c => c.id === conn.to);
                    const isValid = toComp && conn.element && conn.element.parentNode;
                    
                    // 如果連線無效，移除DOM元素
                    if (!isValid && conn.element && conn.element.parentNode) {
                        conn.element.parentNode.removeChild(conn.element);
                    }
                    
                    return isValid;
                });
                
                // 更新有效連線的位置
                comp.connections.forEach(conn => {
                    const toComp = this.components.find(c => c.id === conn.to);
                    if (!toComp || !conn.element) return;
                    
                    // 使用組件的實際位置
                    const x1 = comp.x + 30;
                    const y1 = comp.y + 30;
                    const x2 = toComp.x + 30;
                    const y2 = toComp.y + 30;
                    
                    // 更新連線位置
                    const length = Math.sqrt((x2 - x1) ** 2 + (y2 - y1) ** 2);
                    const angle = Math.atan2(y2 - y1, x2 - x1) * 180 / Math.PI;
                    
                    conn.element.style.left = `${x1}px`;
                    conn.element.style.top = `${y1}px`;
                    conn.element.style.width = `${length}px`;
                    conn.element.style.transform = `rotate(${angle}deg)`;
                });
            }
        } catch (err) {
            console.error("更新連線時發生錯誤:", err, err.stack);
        }
    }
    
    // 移除連線
    removeConnection(comp1, comp2, connectionElement) {
        console.log(`移除連線: ${comp1.id} <-> ${comp2.id}`);
        
        // 從DOM中移除連線元素
        if (connectionElement && connectionElement.parentNode) {
            connectionElement.parentNode.removeChild(connectionElement);
        }
        
        // 從組件的連線列表中移除
        if (comp1.connections) {
            comp1.connections = comp1.connections.filter(c => c.to !== comp2.id);
        }
        if (comp2.connections) {
            comp2.connections = comp2.connections.filter(c => c.to !== comp1.id);
        }
        
        // 顯示移除消息
        this.showMessage(`已移除 ${comp1.id} 與 ${comp2.id} 的連線`, "success");
        
        // 重新計算鏈結構
        this.recalculateAllChains();
    }
}

// 為確保安全，將更新連線的功能也添加到 window 物件用於調試
window.debugUpdateConnections = function() {
    const simulator = document.NucleotideSimulator;
    if (simulator && typeof simulator.updateConnections === 'function') {
        simulator.updateConnections();
    } else {
        console.error("找不到模擬器或更新連線函數");
    }
};

// 初始化模擬器，保留對實例的引用
document.addEventListener('DOMContentLoaded', () => {
    document.NucleotideSimulator = new NucleotideSimulator();
});