#!/usr/bin/env bash
# ============================================================================
# Dream Server Installer — Multi-GPU Strategy Selection
# ============================================================================
# Part of: installers/lib/
# Purpose: Select optimal multi-GPU parallelization strategy based on
#          hardware topology and performance characteristics
#
# Provides: select_multi_gpu_strategy(), calculate_tensor_split(), 
#           get_strategy_config()
#
# Modder notes:
#   This script implements the core strategy selection logic for multi-GPU
#   configurations. It considers GPU count, interconnect type, VRAM, and
#   model size to recommend the optimal parallelization approach.
# ============================================================================

# Strategy selection based on hardware and model characteristics
select_multi_gpu_strategy() {
    local gpu_count="$1"
    local total_vram="$2"      # in MB
    local has_nvlink="$3"      # "true" or "false"
    local model_size="${4:-0}" # in MB, optional
    
    # Validate inputs
    if [[ -z "$gpu_count" || -z "$total_vram" ]]; then
        echo "error:missing_parameters"
        return 1
    fi
    
    # Validate has_nvlink is exactly "true" or "false"
    if [[ "$has_nvlink" != "true" && "$has_nvlink" != "false" ]]; then
        echo "error:invalid_has_nvlink_value:$has_nvlink"
        return 1
    fi
    
    # Single GPU - no strategy needed
    if [[ $gpu_count -eq 1 ]]; then
        echo "single"
        return 0
    fi
    
    # Calculate per-GPU VRAM
    local per_gpu_vram=$((total_vram / gpu_count))
    
    # If model size is known and fits on single GPU, prefer distributed services
    if [[ $model_size -gt 0 && $model_size -lt $per_gpu_vram ]]; then
        echo "distributed"
        return 0
    fi
    
    # High-bandwidth interconnect (NVLink/xGMI) - prefer tensor parallelism
    if [[ "$has_nvlink" == "true" ]]; then
        # For 8+ GPUs with NVLink, hybrid makes more sense
        if [[ $gpu_count -ge 8 ]]; then
            echo "hybrid"
        else
            # For 2-7 GPUs with NVLink, tensor parallelism is optimal
            echo "tensor"
        fi
        return 0
    else
        # 2-3 GPUs on PCIe - pipeline parallelism works well
        if [[ $gpu_count -le 3 ]]; then
            echo "pipeline"
        else
            # 4+ GPUs on PCIe - hybrid approach
            echo "hybrid"
        fi
        return 0
    fi
    
    # Fallback to distributed services
    echo "distributed"
    return 0
}

# Calculate tensor split ratios for heterogeneous GPU setups
calculate_tensor_split() {
    local gpu_vrams="$1" # comma-separated list of VRAM sizes in MB
    
    # Validate input is not empty
    if [[ -z "$gpu_vrams" ]]; then
        echo "error:missing_parameters"
        return 1
    fi
    
    # Split into array
    IFS=',' read -ra vram_array <<< "$gpu_vrams"
    
    # Calculate total VRAM with validation
    local total=0
    for vram in "${vram_array[@]}"; do
        # Validate each element is numeric
        if ! [[ "$vram" =~ ^[0-9]+$ ]]; then
            echo "error:non_numeric_value:$vram"
            return 1
        fi
        total=$((total + vram))
    done
    
    # Guard against zero total
    if [[ $total -eq 0 ]]; then
        echo "error:zero_total_vram"
        return 1
    fi
    
    # Calculate ratios (as percentages)
    local ratios=()
    for vram in "${vram_array[@]}"; do
        local ratio=$(awk "BEGIN {printf \"%.2f\", $vram / $total}")
        ratios+=("$ratio")
    done
    
    # Join with commas
    local IFS=','
    echo "${ratios[*]}"
}

# Get configuration parameters for a given strategy
get_strategy_config() {
    local strategy="$1"
    local gpu_count="$2"
    local gpu_vrams="${3:-}" # optional, comma-separated VRAM sizes
    
    case "$strategy" in
        single)
            cat <<EOF
{
  "strategy": "single",
  "tensor_parallel_size": 1,
  "pipeline_parallel_size": 1,
  "gpu_memory_utilization": 0.95,
  "cuda_visible_devices": "0"
}
EOF
            ;;
        tensor)
            local tensor_split=""
            if [[ -n "$gpu_vrams" ]]; then
                tensor_split=$(calculate_tensor_split "$gpu_vrams")
            fi
            
            cat <<EOF
{
  "strategy": "tensor",
  "tensor_parallel_size": $gpu_count,
  "pipeline_parallel_size": 1,
  "gpu_memory_utilization": 0.92,
  "cuda_visible_devices": "$(seq -s, 0 $((gpu_count - 1)))"$(if [[ -n "$tensor_split" ]]; then echo ",
  \"tensor_split\": \"$tensor_split\""; fi)
}
EOF
            ;;
        pipeline)
            # For pipeline, use all GPUs as stages
            local stages=$gpu_count
            
            cat <<EOF
{
  "strategy": "pipeline",
  "tensor_parallel_size": 1,
  "pipeline_parallel_size": $stages,
  "gpu_memory_utilization": 0.95,
  "cuda_visible_devices": "$(seq -s, 0 $((gpu_count - 1)))"
}
EOF
            ;;
        hybrid)
            # Find largest divisor <= sqrt(gpu_count) that divides evenly
            # This guarantees tensor_size × pipeline_size = gpu_count
            local tensor_size=1
            local sqrt_approx=$(awk "BEGIN {printf \"%.0f\", sqrt($gpu_count)}")
            
            # Find largest divisor from sqrt down to 1
            for ((i=sqrt_approx; i>=1; i--)); do
                if [[ $((gpu_count % i)) -eq 0 ]]; then
                    tensor_size=$i
                    break
                fi
            done
            local pipeline_size=$((gpu_count / tensor_size))
            
            cat <<EOF
{
  "strategy": "hybrid",
  "tensor_parallel_size": $tensor_size,
  "pipeline_parallel_size": $pipeline_size,
  "gpu_memory_utilization": 0.93,
  "cuda_visible_devices": "$(seq -s, 0 $((gpu_count - 1)))"
}
EOF
            ;;
        distributed)
            # Compute GPU assignments in variables before heredoc
            # comfyui uses GPU 1 if we have 2+ GPUs (not 3+)
            local llama_gpu="0"
            local whisper_gpu="0"
            local comfyui_gpu="0"
            local embed_gpu="0"
            
            if [[ $gpu_count -gt 1 ]]; then
                whisper_gpu="$((gpu_count - 1))"
                embed_gpu="$((gpu_count - 1))"
            fi
            
            if [[ $gpu_count -ge 2 ]]; then
                comfyui_gpu="1"
            fi
            
            cat <<EOF
{
  "strategy": "distributed",
  "tensor_parallel_size": 1,
  "pipeline_parallel_size": 1,
  "gpu_memory_utilization": 0.95,
  "service_distribution": {
    "llama_server": "$llama_gpu",
    "whisper": "$whisper_gpu",
    "comfyui": "$comfyui_gpu",
    "embeddings": "$embed_gpu"
  }
}
EOF
            ;;
        *)
            echo "{\"error\": \"unknown_strategy\"}"
            return 1
            ;;
    esac
}

# Get human-readable description of a strategy
get_strategy_description() {
    local strategy="$1"
    
    case "$strategy" in
        single)
            echo "Single GPU - standard configuration"
            ;;
        tensor)
            echo "Tensor Parallelism - model split horizontally across GPUs (best with NVLink)"
            ;;
        pipeline)
            echo "Pipeline Parallelism - model split into sequential stages (works well with PCIe)"
            ;;
        hybrid)
            echo "Hybrid Parallelism - combines tensor and pipeline for optimal throughput"
            ;;
        distributed)
            echo "Distributed Services - different services on different GPUs"
            ;;
        *)
            echo "Unknown strategy"
            ;;
    esac
}

# If this script is run directly (not sourced), show usage
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    echo "Multi-GPU Strategy Selection Library"
    echo "Usage: source this file to use the functions"
    echo ""
    echo "Functions:"
    echo "  select_multi_gpu_strategy <gpu_count> <total_vram_mb> <has_nvlink> [model_size_mb]"
    echo "  calculate_tensor_split <comma_separated_vram_sizes>"
    echo "  get_strategy_config <strategy> <gpu_count> [gpu_vrams]"
    echo "  get_strategy_description <strategy>"
fi
