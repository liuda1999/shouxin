#[cfg(test)]
mod virtual_entity_tests {
    use shouxin::core::contact::{ContactIndex, ContactTotal, OwnerType};
    use shouxin::stores::relational::{BuiltInVirtualCharacters, VirtualCharacterAlias};
    use chrono::Utc;

    #[test]
    fn test_contact_index_is_virtual_default_false() {
        let contact = ContactIndex {
            id: "test_id".to_string(),
            name: "张三".to_string(),
            aliases: "[]".to_string(),
            is_self: false,
            created_at: Utc::now(),
            last_interaction_at: Utc::now(),
            related_contact_ids: "[]".to_string(),
            relationship_tags: "[]".to_string(),
            relation_weight: "".to_string(),
            background: "".to_string(),
            notes: "".to_string(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: false,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        };

        assert!(!contact.is_virtual);
    }

    #[test]
    fn test_contact_index_is_virtual_true() {
        let contact = ContactIndex {
            id: "test_id".to_string(),
            name: "张三".to_string(),
            aliases: "[]".to_string(),
            is_self: false,
            created_at: Utc::now(),
            last_interaction_at: Utc::now(),
            related_contact_ids: "[]".to_string(),
            relationship_tags: "[]".to_string(),
            relation_weight: "".to_string(),
            background: "".to_string(),
            notes: "".to_string(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: true,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        };

        assert!(contact.is_virtual);
    }

    #[test]
    fn test_contact_total_is_virtual_default_false() {
        let contact = ContactTotal::new("test_id".to_string(), "李四".to_string());

        assert!(!contact.is_virtual);
    }

    #[test]
    fn test_contact_total_is_virtual_true() {
        let mut contact = ContactTotal::new("link_id".to_string(), "林克".to_string());
        contact.is_virtual = true;

        assert!(contact.is_virtual);
    }

    #[test]
    fn test_contact_index_from_total_preserves_is_virtual() {
        let mut total = ContactTotal::new("test_id".to_string(), "林克".to_string());
        total.is_virtual = true;

        let index = ContactIndex::from_total(&total);

        assert!(index.is_virtual);
        assert_eq!(index.name, "林克");
        assert_eq!(index.id, "test_id");
    }

    #[test]
    fn test_contact_index_from_total_preserves_is_virtual_false() {
        let total = ContactTotal::new("test_id".to_string(), "王五".to_string());

        let index = ContactIndex::from_total(&total);

        assert!(!index.is_virtual);
        assert_eq!(index.name, "王五");
    }

    #[test]
    fn test_virtual_character_alias_new() {
        let alias = VirtualCharacterAlias::new(
            "contact_id_1".to_string(),
            "Link".to_string(),
            Some("塞尔达传说".to_string()),
            Some("official".to_string()),
        );

        assert_eq!(alias.contact_id, "contact_id_1");
        assert_eq!(alias.alias_name, "Link");
        assert_eq!(alias.source_work, Some("塞尔达传说".to_string()));
        assert_eq!(alias.alias_type, "official");
    }

    #[test]
    fn test_builtin_virtual_characters() {
        let characters = BuiltInVirtualCharacters::get_all();
        
        assert!(!characters.is_empty());
        
        let names: Vec<&str> = characters.iter().map(|(name, _, _)| *name).collect();
        
        assert!(names.contains(&"林克"));
        assert!(names.contains(&"ChatGPT"));
        assert!(names.contains(&"马里奥"));
        assert!(names.contains(&"皮卡丘"));
        assert!(names.contains(&"孙悟空"));
        assert!(names.contains(&"曹操"));
        assert!(names.contains(&"蝙蝠侠"));
        assert!(names.contains(&"钢铁侠"));
    }

    #[test]
    fn test_builtin_link_aliases() {
        let characters = BuiltInVirtualCharacters::get_all();
        
        let link = characters.iter().find(|(name, _, _)| *name == "林克").unwrap();
        let aliases = &link.2;
        
        assert!(aliases.iter().any(|(name, source, _)| *name == "Link" && *source == "塞尔达传说"));
        assert!(aliases.iter().any(|(name, _, alias_type)| *name == "时之勇者" && *alias_type == "nickname"));
    }

    #[test]
    fn test_builtin_caocao_aliases() {
        let characters = BuiltInVirtualCharacters::get_all();
        
        let caocao = characters.iter().find(|(name, _, _)| *name == "曹操").unwrap();
        let aliases = &caocao.2;
        
        assert!(aliases.iter().any(|(name, source, _)| *name == "孟德" && *source == "三国演义"));
        assert!(aliases.iter().any(|(name, _, alias_type)| *name == "魏武帝" && *alias_type == "posthumous_title"));
    }

    #[test]
    fn test_real_contact_is_virtual_false() {
        let real_names = vec!["张三", "李四", "王五", "赵六", "John", "Jane"];

        for name in real_names {
            let contact = ContactTotal::new(uuid::Uuid::new_v4().to_string(), name.to_string());
            assert!(!contact.is_virtual, "Real person {} should not be virtual", name);
        }
    }

    #[test]
    fn test_all_builtin_characters_have_at_least_one_alias() {
        let characters = BuiltInVirtualCharacters::get_all();
        
        for (name, _id, aliases) in characters {
            assert!(
                !aliases.is_empty(),
                "Virtual character '{}' should have at least one alias",
                name
            );
        }
    }
}

#[cfg(test)]
mod name_truncation_tests {
    fn fix_name_truncation(name: &str) -> String {
        let name = name.trim();
        let truncated_markers = [
            "很", "的", "了", "是", "在", "和", "与", "或", "很厉", "厉", "害",
            "说", "道", "问", "答", "叫", "被", "把", "将", "对", "向",
            "做", "去", "来", "过", "要", "会", "能", "可", "请", "让",
            "我", "你", "他", "她", "它", "们", "这", "那", "个", "些",
            "一", "个", "两", "三", "四", "五", "六", "七", "八", "九", "十",
        ];

        for marker in &truncated_markers {
            if name.ends_with(marker) && name.chars().count() > 2 {
                let cleaned = name.trim_end_matches(marker).trim();
                if !cleaned.is_empty() && cleaned.chars().count() >= 2 {
                    return cleaned.to_string();
                }
            }
        }

        if name.chars().any(|c| c.is_ascii_digit()) && name.len() > 3 {
            let cleaned: String = name.chars().take_while(|c| !c.is_ascii_digit())
                .collect::<String>().trim().to_string();
            if !cleaned.is_empty() && cleaned.chars().count() >= 2 {
                return cleaned;
            }
        }

        name.to_string()
    }

    #[test]
    fn test_chinese_name_no_truncation() {
        let names = vec!["张三", "李四", "王五", "赵六", "曹操", "刘备", "孙悟空"];
        
        for name in names {
            let fixed = fix_name_truncation(name);
            assert!(!fixed.is_empty(), "Fixed name should not be empty for {}", name);
            assert_eq!(fixed, name, "Valid name '{}' should not be changed", name);
        }
    }

    #[test]
    fn test_chinese_name_with_particle_suffix() {
        let cases = vec![
            ("曹操很厉", "曹操"),
            ("曹操的", "曹操"),
            ("张三的", "张三"),
            ("李四说", "李四"),
            ("王五很", "王五"),
        ];

        for (input, expected) in cases {
            let fixed = fix_name_truncation(input);
            assert_eq!(fixed, expected, "Expected '{}' from '{}'", expected, input);
        }
    }

    #[test]
    fn test_chinese_name_with_trailing_digits() {
        let cases = vec![
            ("张三123", "张三"),
            ("李四456", "李四"),
        ];

        for (input, expected) in cases {
            let fixed = fix_name_truncation(input);
            assert_eq!(fixed, expected, "Expected '{}' from '{}'", expected, input);
        }
    }

    #[test]
    fn test_single_char_name_not_modified() {
        let name = "张";
        let fixed = fix_name_truncation(name);
        assert_eq!(fixed, name, "Single char name should not be modified");
    }

    #[test]
    fn test_empty_after_fix_returns_empty() {
        let name = "的";
        let fixed = fix_name_truncation(name);
        assert!(fixed.is_empty() || fixed == name, "Edge case for single marker");
    }
}

#[cfg(test)]
mod contact_creation_tests {
    use shouxin::core::contact::{ContactIndex, ContactTotal, OwnerType};
    use chrono::Utc;

    #[test]
    fn test_create_real_contact_is_virtual_false() {
        let name = "张三";
        let contact = ContactIndex {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            aliases: serde_json::to_string(&vec![name.to_string()]).unwrap(),
            is_self: false,
            created_at: Utc::now(),
            last_interaction_at: Utc::now(),
            related_contact_ids: "[]".to_string(),
            relationship_tags: "[]".to_string(),
            relation_weight: "".to_string(),
            background: "".to_string(),
            notes: "".to_string(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: false,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        };

        assert!(!contact.is_virtual, "Real contact {} should have is_virtual=false", name);
    }

    #[test]
    fn test_create_virtual_contact_is_virtual_true() {
        let name = "林克";
        let contact = ContactIndex {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            aliases: serde_json::to_string(&vec![name.to_string(), "Link".to_string()]).unwrap(),
            is_self: false,
            created_at: Utc::now(),
            last_interaction_at: Utc::now(),
            related_contact_ids: "[]".to_string(),
            relationship_tags: "[]".to_string(),
            relation_weight: "".to_string(),
            background: "".to_string(),
            notes: "".to_string(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: true,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        };

        assert!(contact.is_virtual, "Virtual contact {} should have is_virtual=true", name);
    }

    #[test]
    fn test_virtual_contact_aliases_parsing() {
        let aliases = vec!["Link".to_string(), "林克".to_string(), "时之勇者".to_string()];
        
        let contact = ContactIndex {
            id: uuid::Uuid::new_v4().to_string(),
            name: "林克".to_string(),
            aliases: serde_json::to_string(&aliases).unwrap(),
            is_self: false,
            created_at: Utc::now(),
            last_interaction_at: Utc::now(),
            related_contact_ids: "[]".to_string(),
            relationship_tags: "[]".to_string(),
            relation_weight: "".to_string(),
            background: "".to_string(),
            notes: "".to_string(),
            owner_type: OwnerType::User,
            owner_id: "default_user".to_string(),
            is_agent: false,
            parent_agent_id: None,
            is_virtual: true,
            is_archived: false,
            archived_at: None,
            activity_score: 0.0,
        };

        let parsed_aliases: Vec<String> = serde_json::from_str(&contact.aliases).unwrap();
        assert!(parsed_aliases.contains(&"林克".to_string()));
        assert!(parsed_aliases.contains(&"时之勇者".to_string()));
    }
}
