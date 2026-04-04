"""Per-service resource metrics."""

import asyncio
import json
import logging
import urllib.error
import urllib.request
from pathlib import Path

from fastapi import APIRouter, Depends

from config import AGENT_URL, DATA_DIR, DREAM_AGENT_KEY, GPU_BACKEND, SERVICES
from helpers import dir_size_gb
from security import verify_api_key

logger = logging.getLogger(__name__)
router = APIRouter(tags=["resources"])

_DATA_DIR_MAP = {
    "models": "llama-server",
    "qdrant": "qdrant",
    "open-webui": "open-webui",
    "langfuse": "langfuse",
    "n8n": "n8n",
    "comfyui": "comfyui",
    "tts": "tts",
    "whisper": "whisper",
}


def _scan_service_disk() -> dict[str, dict]:
    """Scan /data/* directories and map to services."""
    data_path = Path(DATA_DIR)
    results = {}
    if not data_path.is_dir():
        return results
    for child in data_path.iterdir():
        if not child.is_dir():
            continue
        service_id = _DATA_DIR_MAP.get(child.name, child.name)
        size_gb = dir_size_gb(child)
        if size_gb > 0:
            results[service_id] = {"data_gb": size_gb, "path": f"data/{child.name}"}
    return results


def _fetch_container_stats() -> list[dict]:
    """Fetch container stats from host agent."""
    url = f"{AGENT_URL}/v1/service/stats"
    headers = {"Authorization": f"Bearer {DREAM_AGENT_KEY}"}
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            data = json.loads(resp.read().decode())
            return data.get("containers", [])
    except (urllib.error.HTTPError, urllib.error.URLError, OSError):
        logger.debug("Could not fetch container stats from host agent")
        return []


@router.get("/api/services/resources")
async def service_resources(api_key: str = Depends(verify_api_key)):
    """Get per-service resource metrics (CPU, RAM, disk)."""
    from main import _cache  # noqa: PLC0415 — deferred import to avoid circular dependency

    container_stats = _cache.get("service_resources_containers")
    disk_usage = _cache.get("service_resources_disk")

    need_containers = container_stats is None
    need_disk = disk_usage is None

    if need_containers or need_disk:
        tasks = []
        if need_containers:
            tasks.append(asyncio.to_thread(_fetch_container_stats))
        if need_disk:
            tasks.append(asyncio.to_thread(_scan_service_disk))

        results = await asyncio.gather(*tasks)
        idx = 0
        if need_containers:
            container_stats = results[idx]
            idx += 1
            _cache.set("service_resources_containers", container_stats, 20)
        if need_disk:
            disk_usage = results[idx]
            _cache.set("service_resources_disk", disk_usage, 60)

    container_stats = container_stats or []
    disk_usage = disk_usage or {}

    # Build reverse map: container_name -> service_id from SERVICES dict.
    # This correctly handles non-standard names (dream-webui -> open-webui)
    # when container_name is populated in SERVICES (by PR E's config.py change).
    # Falls back to dream-{sid} convention when container_name is missing.
    container_to_service = {
        svc.get("container_name", f"dream-{sid}"): sid
        for sid, svc in SERVICES.items()
    }

    stats_by_id = {}
    for stat in container_stats:
        cname = stat.get("container_name", "")
        mapped_id = container_to_service.get(cname, stat.get("service_id", cname))
        stats_by_id[mapped_id] = stat

    services = []
    for service_id, config in SERVICES.items():
        entry = {
            "id": service_id,
            "name": config["name"],
            "container": stats_by_id.get(service_id),
            "disk": disk_usage.get(service_id),
        }
        services.append(entry)

    # Add services with disk data but not in SERVICES dict (orphaned data)
    known_ids = set(SERVICES.keys())
    for sid, disk in disk_usage.items():
        if sid not in known_ids:
            services.append({"id": sid, "name": sid, "container": None, "disk": disk})

    total_cpu = sum(s.get("cpu_percent", 0) for s in container_stats)
    total_mem = sum(s.get("memory_used_mb", 0) for s in container_stats)
    total_disk = sum(d.get("data_gb", 0) for d in disk_usage.values())

    return {
        "services": services,
        "totals": {
            "cpu_percent": round(total_cpu, 1),
            "memory_used_mb": round(total_mem),
            "disk_data_gb": round(total_disk, 2),
        },
        "caveats": {
            "docker_desktop_memory": GPU_BACKEND == "apple",
        },
    }
