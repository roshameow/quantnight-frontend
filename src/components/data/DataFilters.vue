<template>
  <div
    style="background: #f8f8f8; padding: 3px 16px; border-radius: 8px; margin-bottom: 6px"
  >
    <n-grid :cols="24" :x-gap="12" :y-gap="8" item-responsive responsive="screen">
      <!-- Left side: Inputs -->
      <n-grid-item span="22 m:20 l:21">
        <!-- Top row: Collection and Type toggle -->
        <div style="margin-bottom: 12px; display: flex; align-items: center; gap: 12px;">
          <n-select
            :value="modelValue.collection"
            @update:value="updateFilter('collection', $event)"
            :options="configStore.dataFilterOptions"
            placeholder="Collection"
            style="width: 160px"
          />

          <n-radio-group
            :value="modelValue.type"
            @update:value="updateFilter('type', $event)"
            name="alpha-type"
          >
            <n-radio-button value="REGULAR">REGULAR</n-radio-button>
            <n-radio-button value="SUPER">SUPER</n-radio-button>
          </n-radio-group>
        </div>

        <!-- Remaining filters -->
        <n-space align="center" wrap :size="[12, 12]">
          <n-input
            :value="modelValue.searchQuery"
            @update:value="updateFilter('searchQuery', $event)"
            placeholder="🔍 Search Regular"
            clearable
            style="width: 250px"
          />
          <n-input
            :value="modelValue.excludeQuery"
            @update:value="updateFilter('excludeQuery', $event)"
            placeholder="🚫 Exclude"
            clearable
            style="width: 150px"
          />
          <n-input
            :value="modelValue.id"
            @update:value="updateFilter('id', $event)"
            placeholder="ID"
            style="width: 80px"
           
          />
          <n-input
            :value="modelValue.region"
            @update:value="updateFilter('region', $event)"
            placeholder="Region"
            style="width: 100px"
           
          />
          <n-input
            :value="modelValue.delay"
            @update:value="updateFilter('delay', $event)"
            placeholder="Delay"
            style="width: 100px"
           
          />
          <n-input
            :value="modelValue.days"
            @update:value="updateFilter('days', $event)"
            placeholder="Days Before"
            type="number"
            style="width: 130px"
           
          />
          <n-input
            :value="modelValue.minTurnover"
            @update:value="updateFilter('minTurnover', $event)"
            placeholder="Min Turnover (%)"
            type="number"
            style="width: 130px"
           
          />
          <n-input
            :value="modelValue.maxTurnover"
            @update:value="updateFilter('maxTurnover', $event)"
            placeholder="Max Turnover (%)"
            type="number"
            style="width: 130px"
           
          />
          <n-input
            :value="modelValue.minMargin"
            @update:value="updateFilter('minMargin', $event)"
            placeholder="Min Margin (‱)"
            type="number"
            style="width: 130px"
           
          />
          <n-input
            :value="modelValue.minReturn"
            @update:value="updateFilter('minReturn', $event)"
            placeholder="Min Return (‱)"
            type="number"
            style="width: 130px"
           
          />
          <n-select
            :value="modelValue.messages"
            multiple
            clearable
            :virtual-scroll="false"
            @update:value="updateFilter('messages', $event)"
            :options="excludeMessageOptions"
            placeholder="Exclude Messages"
            style="width: 400px"
          />
          <n-select
            :value="modelValue.messagesIn"
            multiple
            clearable
            :virtual-scroll="false"
            @update:value="updateFilter('messagesIn', $event)"
            :options="includeMessageOptions"
            placeholder="Include Messages"
            style="width: 400px"
          />
          <n-select
            :value="modelValue.classificationsIn"
            multiple
            clearable
            :virtual-scroll="false"
            @update:value="updateFilter('classificationsIn', $event)"
            :options="classificationOptions"
            placeholder="Include Classifications"
            style="width: 400px"
          />
          <n-select
            :value="modelValue.selfCategories"
            multiple
            clearable
            :virtual-scroll="false"
            @update:value="updateFilter('selfCategories', $event)"
            :options="categoryOptions"
            placeholder="Category"
            style="width: 250px"
          />
        </n-space>
      </n-grid-item>

      <!-- Right side: Button -->
      <n-grid-item span="2 m:4 l:3" style="text-align: right">
        <n-button type="default" @click="$emit('calculateCorr')" :loading="corrLoading">
          计算 Correlation
        </n-button>
      </n-grid-item>
    </n-grid>
  </div>
</template>

<script setup>
import messagesData from '../../data/messages.json';
import classificationsData from '../../data/classifications.json';
import { useConfigStore } from '../../stores/configStore';

const configStore = useConfigStore();

const props = defineProps({
  modelValue: {
    type: Object,
    required: true,
  },
  corrLoading: {
    type: Boolean,
    default: false,
  },
});

const excludeMessageOptions = messagesData.exclude.map(msg => ({ label: msg, value: msg }));
const includeMessageOptions = messagesData.include.map(msg => ({ label: msg, value: msg }));
const classificationOptions = classificationsData.map(c => ({ label: c.name, value: c.id }));

const categoryOptions = [
  'Analyst', 'Broker', 'Earnings', 'Fundamental', 'Imbalance', 'Insiders', 
  'Institutions', 'Macro', 'Model', 'News', 'Option', 'Other', 
  'Price Volume', 'Risk', 'Sentiment', 'Short Interest', 'Social Media'
].map(c => ({ label: c, value: c }));

const emit = defineEmits(["update:modelValue", "calculateCorr"]);

const updateFilter = (key, value) => {
  emit("update:modelValue", { ...props.modelValue, [key]: value });
};
</script>

<style scoped>
.n-input,
.n-select {
  --n-padding-top: 4px !important;
  --n-padding-bottom: 4px !important;
  --n-padding-left: 8px !important;
  --n-padding-right: 8px !important;
  --n-font-size: 13px !important;
}
</style>
