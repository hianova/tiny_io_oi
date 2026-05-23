/**
 * Palantir-inspired Ontology structures for Slope Risk & Cybernetic feedback loops.
 */

export interface Slope {
  id: string;
  vibrationLevel: number;
  riskScore: number;
  status: "Safe" | "Warning" | "Hazard";
}

export interface SlopeKnowledgeGraph {
  baselineVibration: number;       // Environmental baseline vibration
  knownInterferences: number[];    // Known interference frequencies (e.g. 25Hz train)
  falsePositiveCount: number;      // Count of false positives
  currentThreshold: number;        // Current threshold deployed to Swarm tiny-nodes
}
