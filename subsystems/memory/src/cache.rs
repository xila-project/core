pub trait Cache {
    fn flush_data_cache(&self);
    fn flush_instruction_cache(&self);
}
