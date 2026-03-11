#!/usr/bin/env bash
# ============================================================================
# Dream Server — Multi-GPU Strategy Selection Tests
# ============================================================================
# Part of: tests/
# Purpose: Test multi-GPU strategy selection logic
#
# Usage: ./test-multi-gpu-strategy.sh
# ============================================================================

# Note: Don't use set -e because we're testing error conditions

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STRATEGY_SCRIPT="$SCRIPT_DIR/../installers/lib/multi-gpu-strategy.sh"

source "$SCRIPT_DIR/../installers/lib/constants.sh"

# Counters
TESTS_RUN=0
TESTS_PASSED=0
TESTS_FAILED=0

# Check dependencies
if ! command -v jq &>/dev/null; then
    echo -e "${RED}ERROR: jq is required but not installed${NC}"
    exit 1
fi

# Source the strategy script
source "$STRATEGY_SCRIPT"

# Helper function to run a test
run_test() {
    local test_name="$1"
    local expected="$2"
    local actual="$3"
    
    TESTS_RUN=$((TESTS_RUN + 1))
    
    if [[ "$actual" == "$expected" ]]; then
        echo -e "${GRN}✓ PASS${NC}: $test_name"
        TESTS_PASSED=$((TESTS_PASSED + 1))
    else
        echo -e "${RED}✗ FAIL${NC}: $test_name"
        echo -e "  Expected: $expected"
        echo -e "  Got:      $actual"
        TESTS_FAILED=$((TESTS_FAILED + 1))
    fi
}

# Test: Single GPU should return "single"
test_single_gpu() {
    echo -e "${BLU}Testing: Single GPU strategy${NC}"
    
    local result=$(select_multi_gpu_strategy 1 24000 false)
    run_test "Single GPU" "single" "$result"
}

# Test: 2 GPUs with NVLink should prefer tensor parallelism
test_2gpu_nvlink() {
    echo -e "${BLU}Testing: 2 GPUs with NVLink${NC}"
    
    local result=$(select_multi_gpu_strategy 2 48000 true)
    run_test "2 GPUs with NVLink" "tensor" "$result"
}

# Test: 2 GPUs without NVLink should prefer pipeline parallelism
test_2gpu_pcie() {
    echo -e "${BLU}Testing: 2 GPUs with PCIe${NC}"
    
    local result=$(select_multi_gpu_strategy 2 48000 false)
    run_test "2 GPUs with PCIe" "pipeline" "$result"
}

# Test: 4 GPUs with NVLink should prefer tensor parallelism
test_4gpu_nvlink() {
    echo -e "${BLU}Testing: 4 GPUs with NVLink${NC}"
    
    local result=$(select_multi_gpu_strategy 4 96000 true)
    run_test "4 GPUs with NVLink" "tensor" "$result"
}

# Test: 4 GPUs without NVLink should prefer hybrid
test_4gpu_pcie() {
    echo -e "${BLU}Testing: 4 GPUs with PCIe${NC}"
    
    local result=$(select_multi_gpu_strategy 4 96000 false)
    run_test "4 GPUs with PCIe" "hybrid" "$result"
}

# Test: 8 GPUs with NVLink should prefer hybrid
test_8gpu_nvlink() {
    echo -e "${BLU}Testing: 8 GPUs with NVLink${NC}"
    
    local result=$(select_multi_gpu_strategy 8 640000 true)
    run_test "8 GPUs with NVLink" "hybrid" "$result"
}

# Test: 8 GPUs without NVLink should prefer hybrid
test_8gpu_pcie() {
    echo -e "${BLU}Testing: 8 GPUs with PCIe${NC}"
    
    local result=$(select_multi_gpu_strategy 8 640000 false)
    run_test "8 GPUs with PCIe" "hybrid" "$result"
}

# Test: Small model on multi-GPU should prefer distributed
test_small_model_distributed() {
    echo -e "${BLU}Testing: Small model on multi-GPU (distributed)${NC}"
    
    # 8B model (~4GB) on 2x24GB GPUs should use distributed
    local result=$(select_multi_gpu_strategy 2 48000 false 4000)
    run_test "Small model on multi-GPU" "distributed" "$result"
}

# Test: Large model on multi-GPU should use parallelism
test_large_model_parallelism() {
    echo -e "${BLU}Testing: Large model on multi-GPU (parallelism)${NC}"
    
    # 70B model (~35GB) on 2x24GB GPUs should use pipeline
    local result=$(select_multi_gpu_strategy 2 48000 false 35000)
    run_test "Large model on PCIe multi-GPU" "pipeline" "$result"
    
    # Same model with NVLink should use tensor
    result=$(select_multi_gpu_strategy 2 48000 true 35000)
    run_test "Large model on NVLink multi-GPU" "tensor" "$result"
}

# Test: Tensor split calculation
test_tensor_split() {
    echo -e "${BLU}Testing: Tensor split calculation${NC}"
    
    # Equal GPUs should get equal split
    local split=$(calculate_tensor_split "24000,24000")
    run_test "Equal GPU tensor split" "0.50,0.50" "$split"
    
    # Unequal GPUs should get proportional split
    split=$(calculate_tensor_split "24000,48000")
    run_test "Unequal GPU tensor split (1:2)" "0.33,0.67" "$split"
    
    # Three GPUs
    split=$(calculate_tensor_split "24000,24000,24000")
    run_test "Three equal GPUs tensor split" "0.33,0.33,0.33" "$split"
}

# Test: Strategy configuration generation
test_strategy_config() {
    echo -e "${BLU}Testing: Strategy configuration generation${NC}"
    
    # Test single GPU config
    local config=$(get_strategy_config "single" 1)
    local strategy=$(echo "$config" | jq -r '.strategy')
    run_test "Single GPU config strategy" "single" "$strategy"
    
    local tp_size=$(echo "$config" | jq -r '.tensor_parallel_size')
    run_test "Single GPU tensor parallel size" "1" "$tp_size"
    
    # Test tensor parallelism config
    config=$(get_strategy_config "tensor" 4)
    strategy=$(echo "$config" | jq -r '.strategy')
    run_test "Tensor config strategy" "tensor" "$strategy"
    
    tp_size=$(echo "$config" | jq -r '.tensor_parallel_size')
    run_test "Tensor parallel size for 4 GPUs" "4" "$tp_size"
    
    local cuda_devices=$(echo "$config" | jq -r '.cuda_visible_devices')
    run_test "Tensor CUDA devices for 4 GPUs" "0,1,2,3" "$cuda_devices"
    
    # Test pipeline parallelism config
    config=$(get_strategy_config "pipeline" 4)
    strategy=$(echo "$config" | jq -r '.strategy')
    run_test "Pipeline config strategy" "pipeline" "$strategy"
    
    local pp_size=$(echo "$config" | jq -r '.pipeline_parallel_size')
    run_test "Pipeline parallel size for 4 GPUs" "4" "$pp_size"
    
    # Test hybrid config
    config=$(get_strategy_config "hybrid" 8)
    strategy=$(echo "$config" | jq -r '.strategy')
    run_test "Hybrid config strategy" "hybrid" "$strategy"
    
    tp_size=$(echo "$config" | jq -r '.tensor_parallel_size')
    # sqrt(8) ≈ 2.83, largest divisor <= 2.83 is 2
    run_test "Hybrid tensor parallel size for 8 GPUs" "2" "$tp_size"
    
    pp_size=$(echo "$config" | jq -r '.pipeline_parallel_size')
    # 8 / 2 = 4
    run_test "Hybrid pipeline parallel size for 8 GPUs" "4" "$pp_size"
    
    # Test distributed config
    config=$(get_strategy_config "distributed" 4)
    strategy=$(echo "$config" | jq -r '.strategy')
    run_test "Distributed config strategy" "distributed" "$strategy"
    
    local llama_gpu=$(echo "$config" | jq -r '.service_distribution.llama_server')
    run_test "Distributed llama_server GPU" "0" "$llama_gpu"
    
    local whisper_gpu=$(echo "$config" | jq -r '.service_distribution.whisper')
    run_test "Distributed whisper GPU" "3" "$whisper_gpu"
}

# Test: Tensor split in config
test_tensor_split_in_config() {
    echo -e "${BLU}Testing: Tensor split in configuration${NC}"
    
    # Test with heterogeneous GPUs
    local config=$(get_strategy_config "tensor" 2 "24000,48000")
    local has_split=$(echo "$config" | jq 'has("tensor_split")')
    run_test "Config has tensor_split for heterogeneous GPUs" "true" "$has_split"
    
    local split=$(echo "$config" | jq -r '.tensor_split')
    run_test "Tensor split values" "0.33,0.67" "$split"
}

# Test: Bug 1 - Hybrid split produces correct GPU counts
test_bug1_hybrid_split() {
    echo -e "${BLU}Testing: Bug 1 - Hybrid split GPU allocation${NC}"
    
    # 8 GPUs: should be 2×4 = 8 (sqrt(8)≈2.83, largest divisor <=2.83 is 2)
    local config=$(get_strategy_config "hybrid" 8)
    local tp=$(echo "$config" | jq -r '.tensor_parallel_size')
    local pp=$(echo "$config" | jq -r '.pipeline_parallel_size')
    local product=$((tp * pp))
    run_test "Hybrid 8 GPUs: tensor_parallel_size" "2" "$tp"
    run_test "Hybrid 8 GPUs: pipeline_parallel_size" "4" "$pp"
    run_test "Hybrid 8 GPUs: product equals gpu_count" "8" "$product"
    
    # 6 GPUs: should be 2×3 = 6
    config=$(get_strategy_config "hybrid" 6)
    tp=$(echo "$config" | jq -r '.tensor_parallel_size')
    pp=$(echo "$config" | jq -r '.pipeline_parallel_size')
    product=$((tp * pp))
    run_test "Hybrid 6 GPUs: tensor_parallel_size" "2" "$tp"
    run_test "Hybrid 6 GPUs: pipeline_parallel_size" "3" "$pp"
    run_test "Hybrid 6 GPUs: product equals gpu_count" "6" "$product"
    
    # 12 GPUs: sqrt(12)≈3.46, largest divisor <=3.46 is 3, so 3×4 = 12
    config=$(get_strategy_config "hybrid" 12)
    tp=$(echo "$config" | jq -r '.tensor_parallel_size')
    pp=$(echo "$config" | jq -r '.pipeline_parallel_size')
    product=$((tp * pp))
    run_test "Hybrid 12 GPUs: tensor_parallel_size" "3" "$tp"
    run_test "Hybrid 12 GPUs: pipeline_parallel_size" "4" "$pp"
    run_test "Hybrid 12 GPUs: product equals gpu_count" "12" "$product"
    
    # 4 GPUs: sqrt(4)=2, largest divisor <=2 is 2, so 2×2 = 4
    config=$(get_strategy_config "hybrid" 4)
    tp=$(echo "$config" | jq -r '.tensor_parallel_size')
    pp=$(echo "$config" | jq -r '.pipeline_parallel_size')
    product=$((tp * pp))
    run_test "Hybrid 4 GPUs: tensor_parallel_size" "2" "$tp"
    run_test "Hybrid 4 GPUs: pipeline_parallel_size" "2" "$pp"
    run_test "Hybrid 4 GPUs: product equals gpu_count" "4" "$product"
}

# Test: Bug 2 - comfyui GPU assignment
test_bug2_comfyui_assignment() {
    echo -e "${BLU}Testing: Bug 2 - comfyui GPU assignment${NC}"
    
    # 2 GPUs: comfyui should be on GPU 1, not 0
    local config=$(get_strategy_config "distributed" 2)
    local comfyui_gpu=$(echo "$config" | jq -r '.service_distribution.comfyui')
    run_test "Distributed 2 GPUs: comfyui on GPU 1" "1" "$comfyui_gpu"
    
    # 1 GPU: comfyui should be on GPU 0 (only option)
    config=$(get_strategy_config "distributed" 1)
    comfyui_gpu=$(echo "$config" | jq -r '.service_distribution.comfyui')
    run_test "Distributed 1 GPU: comfyui on GPU 0" "0" "$comfyui_gpu"
    
    # 3 GPUs: regression guard, comfyui should be on GPU 1
    config=$(get_strategy_config "distributed" 3)
    comfyui_gpu=$(echo "$config" | jq -r '.service_distribution.comfyui')
    run_test "Distributed 3 GPUs: comfyui on GPU 1" "1" "$comfyui_gpu"
}

# Test: Bug 3 - calculate_tensor_split validation
test_bug3_tensor_split_validation() {
    echo -e "${BLU}Testing: Bug 3 - Tensor split input validation${NC}"
    
    # Empty string input
    local result
    result=$(calculate_tensor_split "" 2>&1)
    local exit_code=$?
    run_test "Empty input: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:missing_parameters"* ]] && \
        run_test "Empty input: error message" "contains error" "contains error" || \
        run_test "Empty input: error message" "contains error" "missing"
    
    # Non-numeric element
    result=$(calculate_tensor_split "80000,bad,40000" 2>&1)
    exit_code=$?
    run_test "Non-numeric input: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:non_numeric_value"* ]] && \
        run_test "Non-numeric input: error message" "contains error" "contains error" || \
        run_test "Non-numeric input: error message" "contains error" "missing"
    
    # All-zero input
    result=$(calculate_tensor_split "0,0,0" 2>&1)
    exit_code=$?
    run_test "Zero total VRAM: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:zero_total_vram"* ]] && \
        run_test "Zero total VRAM: error message" "contains error" "contains error" || \
        run_test "Zero total VRAM: error message" "contains error" "missing"
    
    # Valid input - regression guard
    result=$(calculate_tensor_split "80000,40000" 2>&1)
    exit_code=$?
    run_test "Valid input: zero exit code" "0" "$exit_code"
    run_test "Valid input: correct split" "0.67,0.33" "$result"
}

# Test: Bug 4 - Redundant NVLink condition (regression guards)
test_bug4_nvlink_logic() {
    echo -e "${BLU}Testing: Bug 4 - NVLink logic regression${NC}"
    
    # 4 GPUs with NVLink should still return tensor
    local result=$(select_multi_gpu_strategy 4 160000 true)
    run_test "4 GPUs NVLink: tensor strategy" "tensor" "$result"
    
    # 8 GPUs with NVLink should return hybrid
    result=$(select_multi_gpu_strategy 8 640000 true)
    run_test "8 GPUs NVLink: hybrid strategy" "hybrid" "$result"
}

# Test: Bug 5 - Duplicate PCIe branches (regression guards)
test_bug5_pcie_logic() {
    echo -e "${BLU}Testing: Bug 5 - PCIe logic regression${NC}"
    
    # 4 GPUs PCIe should return hybrid
    local result=$(select_multi_gpu_strategy 4 80000 false)
    run_test "4 GPUs PCIe: hybrid strategy" "hybrid" "$result"
    
    # 8 GPUs PCIe should return hybrid
    result=$(select_multi_gpu_strategy 8 160000 false)
    run_test "8 GPUs PCIe: hybrid strategy" "hybrid" "$result"
    
    # 3 GPUs PCIe should return pipeline (regression guard)
    result=$(select_multi_gpu_strategy 3 60000 false)
    run_test "3 GPUs PCIe: pipeline strategy" "pipeline" "$result"
}

# Test: Bug 6 - Invalid has_nvlink value
test_bug6_invalid_has_nvlink() {
    echo -e "${BLU}Testing: Bug 6 - Invalid has_nvlink validation${NC}"
    
    # Empty string
    local result
    result=$(select_multi_gpu_strategy 4 80000 "" 2>&1)
    local exit_code=$?
    run_test "Empty has_nvlink: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:invalid_has_nvlink_value"* ]] && \
        run_test "Empty has_nvlink: error message" "contains error" "contains error" || \
        run_test "Empty has_nvlink: error message" "contains error" "missing"
    
    # "yes" instead of "true"
    result=$(select_multi_gpu_strategy 4 80000 "yes")
    exit_code=$?
    run_test "has_nvlink=yes: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:invalid_has_nvlink_value"* ]] && \
        run_test "has_nvlink=yes: error message" "contains error" "contains error" || \
        run_test "has_nvlink=yes: error message" "contains error" "missing"
    
    # "1" instead of "true"
    result=$(select_multi_gpu_strategy 4 80000 "1")
    exit_code=$?
    run_test "has_nvlink=1: non-zero exit code" "1" "$exit_code"
    [[ "$result" == *"error:invalid_has_nvlink_value"* ]] && \
        run_test "has_nvlink=1: error message" "contains error" "contains error" || \
        run_test "has_nvlink=1: error message" "contains error" "missing"
    
    # "false" should work (regression guard)
    result=$(select_multi_gpu_strategy 4 80000 "false")
    exit_code=$?
    run_test "has_nvlink=false: zero exit code" "0" "$exit_code"
    run_test "has_nvlink=false: valid strategy" "hybrid" "$result"
}

# Test: Edge cases
test_edge_cases() {
    echo -e "${BLU}Testing: Edge cases${NC}"
    
    # Missing parameters should return error
    local result=$(select_multi_gpu_strategy "" "" "")
    run_test "Missing parameters" "error:missing_parameters" "$result"
}

# Test: Strategy descriptions
test_strategy_descriptions() {
    echo -e "${BLU}Testing: Strategy descriptions${NC}"
    
    local desc=$(get_strategy_description "tensor")
    [[ "$desc" == *"Tensor Parallelism"* ]] && \
        run_test "Tensor strategy description" "contains 'Tensor Parallelism'" "contains 'Tensor Parallelism'" || \
        run_test "Tensor strategy description" "contains 'Tensor Parallelism'" "missing"
    
    desc=$(get_strategy_description "pipeline")
    [[ "$desc" == *"Pipeline Parallelism"* ]] && \
        run_test "Pipeline strategy description" "contains 'Pipeline Parallelism'" "contains 'Pipeline Parallelism'" || \
        run_test "Pipeline strategy description" "contains 'Pipeline Parallelism'" "missing"
    
    desc=$(get_strategy_description "hybrid")
    [[ "$desc" == *"Hybrid"* ]] && \
        run_test "Hybrid strategy description" "contains 'Hybrid'" "contains 'Hybrid'" || \
        run_test "Hybrid strategy description" "contains 'Hybrid'" "missing"
    
    desc=$(get_strategy_description "distributed")
    [[ "$desc" == *"Distributed"* ]] && \
        run_test "Distributed strategy description" "contains 'Distributed'" "contains 'Distributed'" || \
        run_test "Distributed strategy description" "contains 'Distributed'" "missing"
}

# Main test runner
echo -e "${MAG}=== Multi-GPU Strategy Selection Tests ===${NC}\n"

test_single_gpu
test_2gpu_nvlink
test_2gpu_pcie
test_4gpu_nvlink
test_4gpu_pcie
test_8gpu_nvlink
test_8gpu_pcie
test_small_model_distributed
test_large_model_parallelism
test_tensor_split
test_strategy_config
test_tensor_split_in_config
test_bug1_hybrid_split
test_bug2_comfyui_assignment
test_bug3_tensor_split_validation
test_bug4_nvlink_logic
test_bug5_pcie_logic
test_bug6_invalid_has_nvlink
test_edge_cases
test_strategy_descriptions

echo -e "\n${MAG}=== Test Summary ===${NC}"
echo -e "Tests run:    $TESTS_RUN"
echo -e "${GRN}Tests passed: $TESTS_PASSED${NC}"
if [[ $TESTS_FAILED -gt 0 ]]; then
    echo -e "${RED}Tests failed: $TESTS_FAILED${NC}"
    exit 1
else
    echo -e "${GRN}All tests passed!${NC}"
    exit 0
fi
