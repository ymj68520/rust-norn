# Docker Multi-Node Deployment

This directory contains all the files needed to deploy a multi-node Norn blockchain network using Docker.

## Quick Start

```bash
# Start the 4-node network
./docker/start-nodes.sh

# Check node status
./docker/test-network.sh

# View logs
./docker/logs.sh

# Stop the network
./docker/stop-nodes.sh
```

## Directory Structure

```
docker/
├── configs/           # Node configuration files
│   ├── node1.toml     # Bootstrap node config
│   ├── node2.toml     # Node 2 config
│   ├── node3.toml     # Node 3 config
│   └── node4.toml     # Node 4 config
├── scripts/           # Helper scripts
│   └── entrypoint.sh  # Container startup script
├── docker-compose.yml # Multi-node orchestration
├── Dockerfile         # Container image definition
├── start-nodes.sh     # Start the network
├── stop-nodes.sh      # Stop the network
├── logs.sh            # View node logs
├── test-network.sh    # Test network connectivity
└── README.md          # This file
```

## Network Architecture

```
                    ┌─────────────────────────────────────────────┐
                    │           Docker Network (172.28.0.0/16)    │
                    │                                             │
   Host:4001  ──────┼──► Node 1 (Bootstrap) ◄───────────────────┐ │
   Host:50051 ──────┼──► 172.28.0.10:50051                      │ │
                    │         ▲                                  │ │
                    │         │ bootstrap                        │ │
                    │    ┌────┴────┬───────────┐                 │ │
                    │    │         │           │                 │ │
   Host:4002  ──────┼──► Node 2    Node 3      Node 4 ◄─────────┘ │
   Host:50052 ──────┼──► 172.28.0.11          172.28.0.13         │
                    │                                             │
   Host:4003  ──────┼──► Node 3                                   │
   Host:50053 ──────┼──► 172.28.0.12                              │
                    │                                             │
   Host:4004  ──────┼──► Node 4                                   │
   Host:50054 ──────┼──► 172.28.0.13                              │
                    │                                             │
                    └─────────────────────────────────────────────┘
```

## Port Mapping

| Node | Container P2P | Host P2P | Container RPC | Host RPC |
|------|---------------|----------|---------------|----------|
| Node 1 (Bootstrap) | 4001 | 4001 | 50051 | 50051 |
| Node 2 | 4002 | 4002 | 50051 | 50052 |
| Node 3 | 4003 | 4003 | 50051 | 50053 |
| Node 4 | 4004 | 4004 | 50051 | 50054 |

## Commands

### Start Network

```bash
./docker/start-nodes.sh
```

This will:
1. Build the Docker image
2. Create the Docker network
3. Start all 4 nodes
4. Display the network status

### Stop Network

```bash
# Stop nodes but keep data
./docker/stop-nodes.sh

# Stop nodes and remove all data
./docker/stop-nodes.sh --clean
```

### View Logs

```bash
# All nodes
./docker/logs.sh

# Specific node
./docker/logs.sh norn-node1

# Follow mode
./docker/logs.sh -f
```

### Test Connectivity

```bash
./docker/test-network.sh
```

## Configuration

Each node has its own configuration file in `docker/configs/`. Key settings:

- **Bootstrap node (node1.toml)**: No `bootstrap_peers`, other nodes connect to it
- **Other nodes (node2-4.toml)**: Configure `bootstrap_peers` to connect to node1
- **mDNS disabled**: Docker networking doesn't support mDNS reliably

### Adding More Nodes

1. Copy an existing config file:
   ```bash
   cp docker/configs/node4.toml docker/configs/node5.toml
   ```

2. Update the new config:
   - Change `listen_address` port (e.g., `/ip4/0.0.0.0/tcp/4005`)

3. Add the new service to `docker-compose.yml`:
   ```yaml
   norn-node5:
     build:
       context: ..
       dockerfile: docker/Dockerfile
     container_name: norn-node5
     hostname: norn-node5
     ports:
       - "4005:4005"
       - "50055:50051"
     volumes:
       - ./configs/node5.toml:/etc/norn/config.toml:ro
       - node5_data:/data
     environment:
       - RUST_LOG=info
       - NODE_NAME=norn-node5
       - WAIT_FOR_NODE=norn-node1:4001
     networks:
       norn-network:
         ipv4_address: 172.28.0.14
     depends_on:
       - norn-node1
   ```

4. Add the volume:
   ```yaml
   volumes:
     node5_data:
       name: norn_node5_data
   ```

## Troubleshooting

### Nodes not connecting

1. Check if bootstrap node is running:
   ```bash
   docker logs norn-node1
   ```

2. Verify network connectivity:
   ```bash
   docker exec norn-node2 nc -z norn-node1 4001
   ```

### Port conflicts

If ports are already in use, modify the host port mappings in `docker-compose.yml`:
```yaml
ports:
  - "14001:4001"  # Different host port
```

### Clean restart

```bash
./docker/stop-nodes.sh --clean
./docker/start-nodes.sh
```
