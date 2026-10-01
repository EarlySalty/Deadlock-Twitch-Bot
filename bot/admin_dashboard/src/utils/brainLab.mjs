// Never replace a missing family's evidence with the dominant family's scores.
export function scoresForBuild(report, build) {
  if (build.family?.id) return report.variant_scores[build.family.id] ?? [];
  return build === report.build ? report.scored : [];
}
