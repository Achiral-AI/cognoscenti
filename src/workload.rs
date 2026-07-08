use crate::core::{Context, Domain, MemoryChunk};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Types of workloads to simulate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkloadType {
    Strategic,
    Technical,
    Creative,
    Episodic,
    Analytical,
}

/// A single event in the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub context: Context,
    pub participants: Vec<String>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum EventType {
    Meeting,
    Chat,
    DocumentCreation,
    BugReport,
    RoadmapUpdate,
    PriorityChange,
    Decision,
}

/// Main workload structure for simulating team interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workload {
    pub workload_type: WorkloadType,
    pub duration_months: u32,
    pub events: Vec<SimulationEvent>,
    pub memory_chunks: Vec<MemoryChunk>,
}

impl Workload {
    pub fn new(workload_type: WorkloadType, duration_months: u32) -> Self {
        Self {
            workload_type,
            duration_months,
            events: Vec::new(),
            memory_chunks: Vec::new(),
        }
    }

    /// Generate a synthetic workload simulating team interactions
    pub fn generate(&mut self) -> anyhow::Result<()> {
        let mut rng = rand::thread_rng();
        let start_date = Utc::now() - Duration::days((self.duration_months * 30) as i64);
        let end_date = Utc::now();
        
        let events_per_day = match self.workload_type {
            WorkloadType::Strategic => 5,
            WorkloadType::Technical => 15,
            WorkloadType::Creative => 8,
            WorkloadType::Episodic => 3,
            WorkloadType::Analytical => 2,
        };

        let mut current_date = start_date;
        while current_date < end_date {
            let daily_events = rng.gen_range(events_per_day..events_per_day + 3);
            
            for _ in 0..daily_events {
                let event = self.generate_event(current_date, &mut rng)?;
                self.events.push(event);
            }
            
            current_date = current_date + Duration::days(1);
        }

        // Convert events to memory chunks
        self.convert_events_to_chunks();
        
        Ok(())
    }

    fn generate_event(&self, date: DateTime<Utc>, rng: &mut impl Rng) -> anyhow::Result<SimulationEvent> {
        let hour = rng.gen_range(9..18);
        let timestamp = date + Duration::hours(hour);
        
        let (event_type, context, content) = match self.workload_type {
            WorkloadType::Strategic => self.generate_strategic_event(rng),
            WorkloadType::Technical => self.generate_technical_event(rng),
            WorkloadType::Creative => self.generate_creative_event(rng),
            WorkloadType::Episodic => self.generate_episodic_event(rng),
            WorkloadType::Analytical => self.generate_analytical_event(rng),
        };

        let participants = self.generate_participants(&self.workload_type, rng);

        Ok(SimulationEvent {
            timestamp,
            event_type,
            context,
            participants,
            content,
        })
    }

    fn generate_strategic_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Decision,
            EventType::RoadmapUpdate,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("High-Level Decisions".to_string()),
            domain: Domain::General,
            conversation_id: Some(format!("decision-{}", rng.gen::<u32>())),
            participants: vec!["decision-maker".to_string()],
        };

        let contents = vec![
            "Discussed Q4 roadmap priorities",
            "Decided to pivot to enterprise market",
            "Reviewed fundraising strategy",
            "Evaluated competitor positioning",
            "Set hiring targets for engineering",
        ];
        let content = contents[rng.gen_range(0..contents.len())].to_string();

        (event_type, context, content)
    }

    fn generate_technical_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Chat,
            EventType::BugReport,
            EventType::DocumentCreation,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Technical Evolution".to_string()),
            domain: Domain::Engineering,
            conversation_id: Some(format!("tech-{}", rng.gen::<u32>())),
            participants: vec!["technical-agent".to_string()],
        };

        let contents = vec![
            "Fixed critical bug in authentication module",
            "Discussed API rate limiting strategy",
            "Reviewed pull request for feature X",
            "Deployed hotfix to production",
            "Optimized database query performance",
        ];
        let content = contents[rng.gen_range(0..contents.len())].to_string();

        (event_type, context, content)
    }

    fn generate_creative_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::DocumentCreation,
            EventType::Chat,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Cross-Domain Patterns".to_string()),
            domain: Domain::Design,
            conversation_id: Some(format!("creative-{}", rng.gen::<u32>())),
            participants: vec!["creative-agent".to_string()],
        };

        let contents = vec![
            "Updated UI mockups for dashboard",
            "Conducted user research interviews",
            "Refined color palette for brand consistency",
            "Created wireframes for new feature",
            "Reviewed accessibility compliance",
        ];
        let content = contents[rng.gen_range(0..contents.len())].to_string();

        (event_type, context, content)
    }

    fn generate_episodic_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Chat,
            EventType::Meeting,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Short-Term Interactions".to_string()),
            domain: Domain::General,
            conversation_id: Some(format!("episodic-{}", rng.gen::<u32>())),
            participants: vec!["episodic-agent".to_string()],
        };

        let contents = vec![
            "Reported issue with login flow",
            "Requested feature for export functionality",
            "Provided feedback on user experience",
            "Asked about pricing plans",
            "Reported integration problems",
        ];
        let content = contents[rng.gen_range(0..contents.len())].to_string();

        (event_type, context, content)
    }

    fn generate_analytical_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Decision,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Precision Analysis".to_string()),
            domain: Domain::Finance,
            conversation_id: Some(format!("analytical-{}", rng.gen::<u32>())),
            participants: vec!["analytical-agent".to_string()],
        };

        let contents = vec![
            "Discussed term sheet for Series A",
            "Reviewed monthly metrics and KPIs",
            "Evaluated market opportunity",
            "Discussed competitive landscape",
            "Reviewed burn rate and runway",
        ];
        let content = contents[rng.gen_range(0..contents.len())].to_string();

        (event_type, context, content)
    }

    fn generate_participants(&self, workload_type: &WorkloadType, rng: &mut impl Rng) -> Vec<String> {
        match workload_type {
            WorkloadType::Strategic => vec!["decision-maker".to_string()],
            WorkloadType::Technical => (0..rng.gen_range(1..4)).map(|_| format!("tech-agent-{}", rng.gen::<u32>())).collect(),
            WorkloadType::Creative => (0..rng.gen_range(1..3)).map(|_| format!("creative-agent-{}", rng.gen::<u32>())).collect(),
            WorkloadType::Episodic => vec!["episodic-agent".to_string()],
            WorkloadType::Analytical => vec!["analytical-agent".to_string()],
        }
    }

    fn convert_events_to_chunks(&mut self) {
        for (i, event) in self.events.iter().enumerate() {
            let chunk = MemoryChunk::new(
                format!("chunk-{}", i),
                event.content.clone(),
                event.context.clone(),
            );
            self.memory_chunks.push(chunk);
        }
    }

    pub fn get_context_switching_points(&self) -> Vec<(DateTime<Utc>, Context, Context)> {
        let mut switches = Vec::new();
        if self.events.len() < 2 {
            return switches;
        }

        for i in 1..self.events.len() {
            if self.events[i].context.domain != self.events[i-1].context.domain {
                switches.push((
                    self.events[i].timestamp,
                    self.events[i-1].context.clone(),
                    self.events[i].context.clone(),
                ));
            }
        }

        switches
    }
}
