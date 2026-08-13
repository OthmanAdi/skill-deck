<!--
  @agent-context: The one Spinner (DESIGN 7). Renders the shared `.u-spinner`
  utility from app.css — an accent arc riding a hairline track. Replaces the
  ad-hoc `.spin` rings and the inline `refresh-spin` animations scattered across
  DiffModal / RegistryTab / SkillCard / Overlay / SkillRow / FullSkillModal.
  Size and stroke weight are props (fed to the CSS via custom properties).
-->
<script lang="ts">
  let {
    size = 14,
    weight = 2,
    label = "Loading",
    class: klass = "",
  }: {
    size?: number;
    weight?: number;
    /** Accessible name; the spinner is role="status". */
    label?: string;
    class?: string;
  } = $props();
</script>

<span
  class="u-spinner {klass}"
  style="--spinner-size: {size}px; --spinner-weight: {weight}px;"
  role="status"
  aria-label={label}
></span>
