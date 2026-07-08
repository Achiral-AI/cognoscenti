use crate::core::{Context, Domain, MemoryChunk};
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Types of workloads to simulate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkloadType {
    Founders,
    Engineers,
    Designers,
    Customers,
    Investors,
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
            WorkloadType::Founders => 5,
            WorkloadType::Engineers => 15,
            WorkloadType::Designers => 8,
            WorkloadType::Customers => 3,
            WorkloadType::Investors => 2,
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
            WorkloadType::Founders => self.generate_founders_event(rng),
            WorkloadType::Engineers => self.generate_engineers_event(rng),
            WorkloadType::Designers => self.generate_designers_event(rng),
            WorkloadType::Customers => self.generate_customers_event(rng),
            WorkloadType::Investors => self.generate_investors_event(rng),
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

    fn generate_founders_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Decision,
            EventType::RoadmapUpdate,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Strategic Planning".to_string()),
            domain: Domain::General,
            conversation_id: Some(format!("conv-{}", rng.gen::<u32>())),
            participants: vec!["CEO".to_string(), "CTO".to_string()],
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

    fn generate_engineers_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Chat,
            EventType::BugReport,
            EventType::DocumentCreation,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Product Development".to_string()),
            domain: Domain::Engineering,
            conversation_id: Some(format!("eng-{}", rng.gen::<u32>())),
            participants: vec!["Engineer".to_string()],
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

    fn generate_designers_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::DocumentCreation,
            EventType::Chat,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Product Design".to_string()),
            domain: Domain::Design,
            conversation_id: Some(format!("design-{}", rng.gen::<u32>())),
            participants: vec!["Designer".to_string()],
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

    fn generate_customers_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Chat,
            EventType::Meeting,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Customer Support".to_string()),
            domain: Domain::General,
            conversation_id: Some(format!("cust-{}", rng.gen::<u32>())),
            participants: vec!["Customer".to_string()],
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

    fn generate_investors_event(&self, rng: &mut impl Rng) -> (EventType, Context, String) {
        let events = [
            EventType::Meeting,
            EventType::Decision,
        ];
        let event_type = events[rng.gen_range(0..events.len())];
        
        let context = Context {
            project: Some("Fundraising".to_string()),
            domain: Domain::Finance,
            conversation_id: Some(format!("investor-{}", rng.gen::<u32>())),
            participants: vec!["Investor".to_string()],
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
            WorkloadType::Founders => vec!["CEO".to_string(), "CTO".to_string()],
            WorkloadType::Engineers => (0..rng.gen_range(1..4)).map(|_| format!("Engineer-{}", rng.gen::<u32>())).collect(),
            WorkloadType::Designers => (0..rng.gen_range(1..3)).map(|_| format!("Designer-{}", rng.gen::<u32>())).collect(),
            WorkloadType::Customers => vec!["Customer".to_string()],
            WorkloadType::Investors => vec!["Investor".to_string()],
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
