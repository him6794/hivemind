# Web Service Module Documentation

## 📋 Overview

The Web Service Module is the frontend interface layer of the HiveMind distributed computing platform, providing a complete web management interface, VPN service management, and WireGuard server integration. Built on the Flask framework, it offers users an intuitive graphical management interface.

## 🏗️ System Architecture

```
┌─────────────────────┐
│   Web Service       │
│     Module          │
├─────────────────────┤
│ • Flask Application │
│ • VPN Management    │
│ • WireGuard Server  │
│ • Web Interface     │
│ • Static Resources  │
└─────────────────────┘
        │
        ├─ HTTP/HTTPS Protocol
        ├─ VPN Network Management
        ├─ User Authentication
        └─ System Monitoring Interface
```

## 🔧 Core Components

### 1. Flask Application (`app.py`)
- **Function**: Main web application
- **Status**: Core functionality complete
- **Purpose**: Provide RESTful API and web interface

**Main Features**:
```python
from flask import Flask, render_template, request, jsonify, session
from werkzeug.security import generate_password_hash, check_password_hash
import logging
import os

app = Flask(__name__)
app.secret_key = os.environ.get('SECRET_KEY', 'hivemind-secret-key')

# Configure logging
logging.basicConfig(level=logging.INFO)
logger = logging.getLogger(__name__)

class HiveMindWebApp:
    def __init__(self):
        self.app = app
        self.setup_routes()
        self.setup_error_handlers()
        
    def setup_routes(self):
        """Setup routes"""
        
        @self.app.route('/')
        def index():
            """Homepage"""
            return render_template('index.html')
        
        @self.app.route('/dashboard')
        def dashboard():
            """Management dashboard"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # Get system status
            system_status = self.get_system_status()
            node_stats = self.get_node_statistics()
            
            return render_template('dashboard.html', 
                                 system_status=system_status,
                                 node_stats=node_stats)
        
        @self.app.route('/nodes')
        def nodes():
            """Node management"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # Get all node information
            nodes = self.get_all_nodes()
            
            return render_template('nodes.html', nodes=nodes)
        
        @self.app.route('/tasks')
        def tasks():
            """Task management"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # Get task list
            tasks = self.get_task_list()
            
            return render_template('tasks.html', tasks=tasks)
        
        @self.app.route('/vpn')
        def vpn_management():
            """VPN management interface"""
            if 'user_id' not in session:
                return redirect('/login')
            
            # Get VPN status
            vpn_status = self.get_vpn_status()
            clients = self.get_vpn_clients()
            
            return render_template('vpn.html', 
                                 vpn_status=vpn_status,
                                 clients=clients)
        
        @self.app.route('/api/system/status')
        def api_system_status():
            """System status API"""
            try:
                status = {
                    'cpu_usage': self.get_cpu_usage(),
                    'memory_usage': self.get_memory_usage(),
                    'disk_usage': self.get_disk_usage(),
                    'network_status': self.get_network_status(),
                    'active_nodes': self.get_active_node_count(),
                    'running_tasks': self.get_running_task_count(),
                    'vpn_status': self.get_vpn_service_status()
                }
                return jsonify(status)
            except Exception as e:
                logger.error(f"Error getting system status: {e}")
                return jsonify({'error': str(e)}), 500
        
        @self.app.route('/api/nodes', methods=['GET'])
        def api_get_nodes():
            """Get node list API"""
            try:
                nodes = self.get_all_nodes()
                return jsonify(nodes)
            except Exception as e:
                logger.error(f"Error getting nodes: {e}")
                return jsonify({'error': str(e)}), 500
        
        @self.app.route('/api/tasks', methods=['GET', 'POST'])
        def api_tasks():
            """Task management API"""
            if request.method == 'GET':
                try:
                    tasks = self.get_task_list()
                    return jsonify(tasks)
                except Exception as e:
                    logger.error(f"Error getting tasks: {e}")
                    return jsonify({'error': str(e)}), 500
            
            elif request.method == 'POST':
                try:
                    task_data = request.get_json()
                    task_id = self.create_task(task_data)
                    return jsonify({'task_id': task_id, 'status': 'created'})
                except Exception as e:
                    logger.error(f"Error creating task: {e}")
                    return jsonify({'error': str(e)}), 500
    
    def get_system_status(self):
        """Get system status"""
        import psutil
        
        return {
            'cpu_percent': psutil.cpu_percent(interval=1),
            'memory_percent': psutil.virtual_memory().percent,
            'disk_percent': psutil.disk_usage('/').percent,
            'boot_time': psutil.boot_time(),
            'load_average': os.getloadavg() if hasattr(os, 'getloadavg') else [0, 0, 0]
        }
    
    def start_server(self, host='0.0.0.0', port=5000, debug=False):
        """Start web server"""
        logger.info(f"Starting HiveMind Web Server on {host}:{port}")
        self.app.run(host=host, port=port, debug=debug)

# Create application instance
web_app = HiveMindWebApp()

if __name__ == '__main__':
    web_app.start_server(debug=True)
```

### 2. VPN Service Management (`vpn_service.py`)
- **Function**: VPN network management service
- **Status**: Basic implementation complete
- **Protocol**: WireGuard VPN

**Implementation Example**:
```python
import subprocess
import json
import os
from datetime import datetime, timedelta
import ipaddress

class VPNService:
    def __init__(self, config_path='/etc/wireguard'):
        self.config_path = config_path
        self.interface_name = 'wg0'
        self.server_port = 51820
        self.network_range = '10.0.1.0/24'
        self.allocated_ips = set()
        
    def initialize_server(self):
        """Initialize VPN server"""
        try:
            # Generate server key pair
            private_key = self._generate_private_key()
            public_key = self._generate_public_key(private_key)
            
            # Create server configuration
            server_config = self._create_server_config(private_key)
            
            # Save configuration file
            config_file = os.path.join(self.config_path, f'{self.interface_name}.conf')
            with open(config_file, 'w') as f:
                f.write(server_config)
            
            # Start WireGuard interface
            self._start_wireguard_interface()
            
            return {
                'status': 'success',
                'public_key': public_key,
                'endpoint': f'{self._get_server_ip()}:{self.server_port}',
                'network': self.network_range
            }
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
    
    def create_client_config(self, client_name, client_email=None):
        """Create client configuration"""
        try:
            # Generate client key pair
            client_private_key = self._generate_private_key()
            client_public_key = self._generate_public_key(client_private_key)
            
            # Allocate IP address
            client_ip = self._allocate_client_ip()
            
            # Get server public key
            server_public_key = self._get_server_public_key()
            
            # Create client configuration
            client_config = f"""[Interface]
PrivateKey = {client_private_key}
Address = {client_ip}/32
DNS = 8.8.8.8, 8.8.4.4

[Peer]
PublicKey = {server_public_key}
Endpoint = {self._get_server_ip()}:{self.server_port}
AllowedIPs = 0.0.0.0/0
PersistentKeepalive = 25
"""
            
            # Add client to server configuration
            self._add_client_to_server(client_name, client_public_key, client_ip)
            
            # Save client information
            client_info = {
                'name': client_name,
                'email': client_email,
                'public_key': client_public_key,
                'ip_address': client_ip,
                'created_at': datetime.now().isoformat(),
                'last_handshake': None,
                'bytes_sent': 0,
                'bytes_received': 0
            }
            
            self._save_client_info(client_name, client_info)
            
            return {
                'status': 'success',
                'config': client_config,
                'client_info': client_info
            }
            
        except Exception as e:
            return {'status': 'error', 'message': str(e)}
```

### 3. WireGuard Server (`wireguard_server.py`)
- **Function**: WireGuard VPN server management
- **Status**: Core functionality implemented
- **Purpose**: Provide secure point-to-point VPN connections

**Implementation Example**:
```python
import os
import subprocess
import json
import logging
from datetime import datetime

class WireGuardServer:
    def __init__(self, config_dir='/etc/wireguard', interface='wg0'):
        self.config_dir = config_dir
        self.interface = interface
        self.config_file = os.path.join(config_dir, f'{interface}.conf')
        self.clients_dir = os.path.join(config_dir, 'clients')
        
        # Ensure directories exist
        os.makedirs(config_dir, exist_ok=True)
        os.makedirs(self.clients_dir, exist_ok=True)
        
        # Setup logging
        self.logger = logging.getLogger(__name__)
    
    def setup_server(self, server_ip='10.0.1.1/24', port=51820):
        """Setup WireGuard server"""
        try:
            # Generate server keys
            server_private_key = self._generate_key()
            server_public_key = self._get_public_key(server_private_key)
            
            # Create server configuration
            config_content = f"""[Interface]
PrivateKey = {server_private_key}
Address = {server_ip}
ListenPort = {port}
PostUp = iptables -A FORWARD -i %i -j ACCEPT; iptables -A FORWARD -o %i -j ACCEPT; iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
PostDown = iptables -D FORWARD -i %i -j ACCEPT; iptables -D FORWARD -o %i -j ACCEPT; iptables -t nat -D POSTROUTING -o eth0 -j MASQUERADE

"""
            
            # Save configuration file
            with open(self.config_file, 'w') as f:
                f.write(config_content)
            
            # Set file permissions
            os.chmod(self.config_file, 0o600)
            
            # Enable IP forwarding
            self._enable_ip_forwarding()
            
            # Start WireGuard interface
            self.start_interface()
            
            self.logger.info(f"WireGuard server setup completed on {self.interface}")
            
            return {
                'success': True,
                'public_key': server_public_key,
                'endpoint': f'{self._get_public_ip()}:{port}',
                'network': server_ip
            }
            
        except Exception as e:
            self.logger.error(f"Error setting up WireGuard server: {e}")
            return {'success': False, 'error': str(e)}
    
    def add_peer(self, peer_name, peer_ip, allowed_ips='10.0.1.0/24'):
        """Add peer"""
        try:
            # Generate peer keys
            peer_private_key = self._generate_key()
            peer_public_key = self._get_public_key(peer_private_key)
            
            # Create peer configuration file
            peer_config = self._create_peer_config(
                peer_private_key, peer_ip, allowed_ips
            )
            
            # Save peer configuration
            peer_config_file = os.path.join(self.clients_dir, f'{peer_name}.conf')
            with open(peer_config_file, 'w') as f:
                f.write(peer_config)
            
            # Add peer to server configuration
            self._add_peer_to_server_config(peer_name, peer_public_key, peer_ip)
            
            # Reload configuration
            self.reload_config()
            
            self.logger.info(f"Added peer {peer_name} with IP {peer_ip}")
            
            return {
                'success': True,
                'peer_name': peer_name,
                'public_key': peer_public_key,
                'config_file': peer_config_file,
                'config_content': peer_config
            }
            
        except Exception as e:
            self.logger.error(f"Error adding peer {peer_name}: {e}")
            return {'success': False, 'error': str(e)}
```

## 🗂️ File Structure

```
web/
├── app.py                     # Flask main application
├── vpn_service.py            # VPN service management
├── wireguard_server.py       # WireGuard server
├── __pycache__/              # Python cache
├── static/                   # Static resources
│   ├── css/                  # Stylesheets
│   ├── js/                   # JavaScript files
│   └── images/               # Image resources
└── templates/                # HTML templates
    ├── base.html             # Base template
    ├── index.html            # Homepage
    ├── dashboard.html        # Dashboard
    ├── nodes.html            # Node management
    ├── tasks.html            # Task management
    └── vpn.html              # VPN management
```

## 🌐 Web Interface Features

### 1. System Dashboard
```html
<!-- dashboard.html example -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>HiveMind Management Dashboard</title>
    <link rel="stylesheet" href="{{ url_for('static', filename='css/dashboard.css') }}">
</head>
<body>
    <div class="container">
        <header>
            <h1>HiveMind Distributed Computing Platform</h1>
            <nav>
                <a href="/dashboard">Dashboard</a>
                <a href="/nodes">Node Management</a>
                <a href="/tasks">Task Management</a>
                <a href="/vpn">VPN Management</a>
            </nav>
        </header>
        
        <main>
            <div class="stats-grid">
                <div class="stat-card">
                    <h3>System Status</h3>
                    <div class="metric">
                        <span class="label">CPU Usage:</span>
                        <span class="value">{{ system_status.cpu_percent }}%</span>
                    </div>
                    <div class="metric">
                        <span class="label">Memory Usage:</span>
                        <span class="value">{{ system_status.memory_percent }}%</span>
                    </div>
                </div>
                
                <div class="stat-card">
                    <h3>Node Statistics</h3>
                    <div class="metric">
                        <span class="label">Total Nodes:</span>
                        <span class="value">{{ node_stats.total_nodes }}</span>
                    </div>
                    <div class="metric">
                        <span class="label">Active Nodes:</span>
                        <span class="value">{{ node_stats.active_nodes }}</span>
                    </div>
                </div>
            </div>
            
            <div class="charts-section">
                <div class="chart-container">
                    <canvas id="cpuChart"></canvas>
                </div>
                <div class="chart-container">
                    <canvas id="networkChart"></canvas>
                </div>
            </div>
        </main>
    </div>
    
    <script src="{{ url_for('static', filename='js/dashboard.js') }}"></script>
</body>
</html>
```

### 2. Node Management Interface
```javascript
// static/js/nodes.js
class NodeManager {
    constructor() {
        this.nodes = [];
        this.init();
    }
    
    init() {
        this.loadNodes();
        this.setupEventListeners();
        this.startAutoRefresh();
    }
    
    async loadNodes() {
        try {
            const response = await fetch('/api/nodes');
            this.nodes = await response.json();
            this.renderNodes();
        } catch (error) {
            console.error('Error loading nodes:', error);
        }
    }
    
    renderNodes() {
        const container = document.getElementById('nodes-container');
        container.innerHTML = '';
        
        this.nodes.forEach(node => {
            const nodeCard = this.createNodeCard(node);
            container.appendChild(nodeCard);
        });
    }
    
    createNodeCard(node) {
        const card = document.createElement('div');
        card.className = `node-card status-${node.status}`;
        
        card.innerHTML = `
            <div class="node-header">
                <h3>${node.hostname}</h3>
                <span class="status-badge ${node.status}">${node.status}</span>
            </div>
            <div class="node-details">
                <p><strong>IP:</strong> ${node.ip_address}</p>
                <p><strong>CPU:</strong> ${node.cpu_usage}%</p>
                <p><strong>Memory:</strong> ${node.memory_usage}%</p>
                <p><strong>Last Seen:</strong> ${this.formatTime(node.last_seen)}</p>
            </div>
            <div class="node-actions">
                <button onclick="nodeManager.viewNode('${node.node_id}')">View Details</button>
                <button onclick="nodeManager.restartNode('${node.node_id}')">Restart Node</button>
            </div>
        `;
        
        return card;
    }
    
    async restartNode(nodeId) {
        try {
            const response = await fetch(`/api/nodes/${nodeId}/restart`, {
                method: 'POST'
            });
            
            if (response.ok) {
                alert('Node restart command sent');
                this.loadNodes();
            } else {
                alert('Failed to restart node');
            }
        } catch (error) {
            console.error('Error restarting node:', error);
            alert('Error occurred while restarting node');
        }
    }
    
    formatTime(timestamp) {
        return new Date(timestamp).toLocaleString();
    }
    
    startAutoRefresh() {
        setInterval(() => {
            this.loadNodes();
        }, 30000); // Refresh every 30 seconds
    }
}

// Initialize node manager
const nodeManager = new NodeManager();
```

## 📊 Monitoring and Metrics

### Web Service Metrics Collection
```python
class WebMetricsCollector:
    def __init__(self):
        self.metrics = {
            'requests': 0,
            'response_times': [],
            'active_sessions': 0,
            'vpn_connections': 0,
            'api_calls': {},
            'error_count': 0
        }
        
    def record_request(self, endpoint, response_time, status_code):
        """Record request metrics"""
        self.metrics['requests'] += 1
        self.metrics['response_times'].append(response_time)
        
        if endpoint not in self.metrics['api_calls']:
            self.metrics['api_calls'][endpoint] = 0
        self.metrics['api_calls'][endpoint] += 1
        
        if status_code >= 400:
            self.metrics['error_count'] += 1
    
    def get_performance_metrics(self):
        """Get performance metrics"""
        avg_response_time = 0
        if self.metrics['response_times']:
            avg_response_time = sum(self.metrics['response_times']) / len(self.metrics['response_times'])
        
        return {
            'total_requests': self.metrics['requests'],
            'avg_response_time_ms': avg_response_time,
            'active_sessions': self.metrics['active_sessions'],
            'vpn_connections': self.metrics['vpn_connections'],
            'error_rate': self.metrics['error_count'] / max(1, self.metrics['requests']),
            'top_endpoints': sorted(
                self.metrics['api_calls'].items(), 
                key=lambda x: x[1], 
                reverse=True
            )[:10]
        }
```

## 🔒 Security Features

### 1. User Authentication System
```python
from werkzeug.security import generate_password_hash, check_password_hash
import jwt
from datetime import datetime, timedelta

class AuthenticationService:
    def __init__(self, secret_key):
        self.secret_key = secret_key
        self.users = {}  # Should use database in production
        
    def register_user(self, username, password, email):
        """Register user"""
        if username in self.users:
            return {'success': False, 'message': 'Username already exists'}
        
        password_hash = generate_password_hash(password)
        
        self.users[username] = {
            'password_hash': password_hash,
            'email': email,
            'created_at': datetime.now(),
            'last_login': None,
            'role': 'user'
        }
        
        return {'success': True, 'message': 'User registered successfully'}
    
    def authenticate_user(self, username, password):
        """User authentication"""
        if username not in self.users:
            return {'success': False, 'message': 'User does not exist'}
        
        user = self.users[username]
        
        if check_password_hash(user['password_hash'], password):
            # Update last login time
            self.users[username]['last_login'] = datetime.now()
            
            # Generate JWT Token
            token = self.generate_token(username)
            
            return {
                'success': True,
                'token': token,
                'user': {
                    'username': username,
                    'email': user['email'],
                    'role': user['role']
                }
            }
        else:
            return {'success': False, 'message': 'Incorrect password'}
    
    def generate_token(self, username):
        """Generate JWT Token"""
        payload = {
            'username': username,
            'exp': datetime.utcnow() + timedelta(hours=24)
        }
        return jwt.encode(payload, self.secret_key, algorithm='HS256')
    
    def verify_token(self, token):
        """Verify JWT Token"""
        try:
            payload = jwt.decode(token, self.secret_key, algorithms=['HS256'])
            return {'success': True, 'username': payload['username']}
        except jwt.ExpiredSignatureError:
            return {'success': False, 'message': 'Token expired'}
        except jwt.InvalidTokenError:
            return {'success': False, 'message': 'Invalid token'}
```

## 🔧 Usage Examples

### Start Web Service
```python
# Start complete web service
from web.app import web_app
from web.vpn_service import VPNService

# Initialize VPN service
vpn_service = VPNService()
vpn_result = vpn_service.initialize_server()

if vpn_result['status'] == 'success':
    print(f"VPN server started: {vpn_result['endpoint']}")

# Start web application
web_app.start_server(host='0.0.0.0', port=5000, debug=False)
```

### Create VPN Client
```python
# Create VPN configuration for new node
client_result = vpn_service.create_client_config('worker-001', 'admin@hivemind.local')

if client_result['status'] == 'success':
    print("Client configuration:")
    print(client_result['config'])
    
    # Save configuration file
    with open('worker-001.conf', 'w') as f:
        f.write(client_result['config'])
```

## 🔧 Common Troubleshooting

### 1. Web Service Startup Failure
**Problem**: Flask application cannot start
**Solution**:
```python
def diagnose_web_service():
    # Check if port is occupied
    import socket
    
    def check_port(port):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        result = sock.connect_ex(('localhost', port))
        sock.close()
        return result == 0
    
    if check_port(5000):
        print("Port 5000 is occupied, please change port")
    else:
        print("Port 5000 is available")
    
    # Check dependencies
    try:
        import flask
        print(f"Flask version: {flask.__version__}")
    except ImportError:
        print("Flask not installed, please run: pip install flask")
```

### 2. VPN Connection Issues
**Problem**: WireGuard VPN cannot connect
**Solution**:
```python
def diagnose_vpn_issues():
    # Check if WireGuard is installed
    import subprocess
    
    try:
        result = subprocess.run(['wg', '--version'], capture_output=True, text=True)
        print(f"WireGuard version: {result.stdout}")
    except FileNotFoundError:
        print("WireGuard not installed")
        return
    
    # Check firewall settings
    print("Checking firewall settings...")
    
    # Check interface status
    try:
        result = subprocess.run(['wg', 'show'], capture_output=True, text=True)
        if result.stdout:
            print("WireGuard interface status:")
            print(result.stdout)
        else:
            print("No active WireGuard interfaces")
    except Exception as e:
        print(f"Error checking interface status: {e}")
```

---

**Related Documentation**:
- [Master Node Module](master-node.md)
- [Node Pool Module](node-pool.md)
- [API Documentation](../api.md)
- [Deployment Guide](../deployment.md)
- [Security Guide](../security.md)
